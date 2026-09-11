//! Opt-in debug integration audit. Operates only its own disposable AXFixture
//! processes, own editor fixtures, and an empty temporary database. Submitted
//! microphone audio is spooled only in that test directory; ASR/polish responses
//! are controlled per UUID to exercise asynchronous ordering without model/network.
use crate::{db, types::PasteMode, voice_input};
use serde_json::{json, Value};
use std::{path::Path, process::Stdio, time::Duration};
use tauri::{AppHandle, Listener, Manager};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

pub(crate) fn prepare_ui(app: &AppHandle) {
    if std::env::var_os("HVR_AX_AUDIT_FIXTURE").is_some()
        && std::env::var_os("VOICE_VIBE_TEST_APP_DATA_DIR").is_some()
    {
        // The full dashboard enumerates screen-capture audio devices on load.
        // This audit needs the recorder/overlay, not ScreenCaptureKit or a meeting.
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.hide();
            let _ = main.navigate("about:blank".parse().unwrap());
        }
    }
}

struct Fixture {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Fixture {
    async fn launch(path: &Path, title: &str, web: bool) -> Result<Self, String> {
        let mut command = Command::new(path);
        command.arg(title).arg("--audit");
        if web {
            command.arg("--web");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| e.to_string())?;
        let input = child.stdin.take().ok_or("fixture stdin")?;
        let output = BufReader::new(child.stdout.take().ok_or("fixture stdout")?);
        let mut fixture = Self {
            child,
            input,
            output,
        };
        let ready = fixture.read().await?;
        if ready["ready"] != true || ready["pid"].as_u64() != fixture.child.id().map(u64::from) {
            return Err("Fixture identity/initialization failed".into());
        }
        Ok(fixture)
    }

    async fn read(&mut self) -> Result<Value, String> {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(15), self.output.read_line(&mut line))
            .await
            .map_err(|_| "fixture timeout")?
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&line).map_err(|e| e.to_string())
    }

    async fn command(&mut self, value: Value) -> Result<Value, String> {
        let mut line = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
        line.push(b'\n');
        self.input
            .write_all(&line)
            .await
            .map_err(|e| e.to_string())?;
        self.input.flush().await.map_err(|e| e.to_string())?;
        self.read().await
    }

    async fn activate(&mut self, app: &AppHandle) -> Result<(), String> {
        self.command(json!({"op":"activate"})).await?;
        for _ in 0..80 {
            let front = super::native::call(app, json!({"op":"frontmost"})).await?;
            if front["pid"].as_u64() == self.child.id().map(u64::from) {
                tokio::time::sleep(Duration::from_millis(100)).await;
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Err("Owned fixture failed to become frontmost".into())
    }
}

fn require(ok: bool, message: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(message.into())
    }
}

async fn record_target(
    app: &AppHandle,
    fixture: &mut Fixture,
    mode: PasteMode,
) -> Result<crate::types::PasteTarget, String> {
    fixture.activate(app).await?;
    db::set_pastebox_mode(mode)?;
    let status = voice_input::start_dictation(app.clone()).await?;
    require(
        status.phase == "listening",
        "Real microphone did not enter listening",
    )?;
    let frozen = voice_input::audit_dictation_context().ok_or("No recording context")?;
    let (captured_mode, target) = frozen.resolve().await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    voice_input::cancel_dictation(app.clone()).await?;
    let target =
        target.ok_or_else(|| "Real recorder did not capture an input target".to_string())?;
    require(captured_mode == mode, "Recording mode changed")?;
    require(
        target.process_id == fixture.child.id().map(i64::from),
        "Recording captured a different app",
    )?;
    let identity = fixture.command(json!({"op":"identity"})).await?;
    require(
        identity["window_id"].as_u64() == target.window_id.map(u64::from),
        "Recording did not retain the fixture's native window ID",
    )?;
    Ok(target)
}

async fn notify_and_wait(id: &str) -> Result<(), String> {
    // Exercise the actual native notification callback, with an existing outbox ID.
    super::handle_native_event(json!({"type":"notification", "id": id}));
    for _ in 0..100 {
        let item = db::get_paste_item(id)?;
        if item.delivery_status == "verified" {
            return Ok(());
        }
        if item.delivery_status == "failed" || item.delivery_status == "sent" {
            return Err(format!("Notification delivery: {:?}", item.error));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("Notification callback timeout".into())
}

async fn self_editor_command(
    app: &AppHandle,
    label: &str,
    mut request: Value,
) -> Result<Value, String> {
    let event = format!("hvr-target-fixture-{}", uuid::Uuid::new_v4());
    request["event"] = json!(event);
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let listener = app.listen(event, move |event| {
        let _ = sender.send(serde_json::from_str::<Value>(event.payload()));
    });
    let window = app
        .get_webview_window(label)
        .ok_or("Missing own test window")?;
    let script = format!("window.hvrTargetFixture?.({request})");
    let result = async {
        // Wait for this test page's modules/editor initialization, without reloading
        // the production dashboard or ever inspecting another app's web content.
        for _ in 0..60 {
            window.eval(&script).map_err(|error| error.to_string())?;
            if let Ok(Some(response)) =
                tokio::time::timeout(Duration::from_millis(200), receiver.recv()).await
            {
                let response = response.map_err(|error| error.to_string())?;
                if response["error"].is_null() {
                    return Ok(response);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        Err("Own editor fixture did not become ready".to_string())
    }
    .await;
    app.unlisten(listener);
    result
}

async fn audit_own_editors(
    app: &AppHandle,
    other_app: &mut Fixture,
    results: &mut Vec<Value>,
) -> Result<(), String> {
    let url = app
        .config()
        .build
        .dev_url
        .as_ref()
        .ok_or("Audit requires Vite")?
        .join("/tests/fixtures/pastebox-self.html")
        .map_err(|e| e.to_string())?;
    let main = app
        .get_webview_window("main")
        .ok_or("Missing own main window")?;
    main.navigate(url.clone()).map_err(|e| e.to_string())?;
    self_editor_command(app, "main", json!({"op":"read", "kind":"plain"})).await?;
    for kind in ["plain", "rich", "source"] {
        main.show().map_err(|e| e.to_string())?;
        let mut activated = false;
        for _ in 0..60 {
            main.set_focus().map_err(|e| e.to_string())?;
            let front = super::native::call(app, json!({"op":"frontmost"})).await?;
            if front["pid"].as_u64() == Some(u64::from(std::process::id())) {
                activated = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        require(activated, "Own main window did not become frontmost")?;
        // Match a real click into the editor: focus the native WKWebView as
        // well as the NSWindow before selecting its DOM text.
        main.as_ref().set_focus().map_err(|e| e.to_string())?;
        self_editor_command(
            app,
            "main",
            json!({"op":"select", "kind":kind, "location":3}),
        )
        .await?;
        tokio::time::sleep(Duration::from_millis(200)).await;
        db::set_pastebox_mode(PasteMode::Automatic)?;
        let status = voice_input::start_dictation(app.clone()).await?;
        require(
            status.phase == "listening",
            "Own editor recorder failed to start",
        )?;
        let (_, target) = voice_input::audit_dictation_context()
            .ok_or("No own recording context")?
            .resolve()
            .await;
        let target = target.ok_or_else(|| format!("Own {kind} editor was not captured"))?;
        require(
            target.process_id == Some(i64::from(std::process::id())),
            "Own target PID mismatch",
        )?;
        require(
            target.selection_location == 3 && target.selection_length == 0,
            "Own editor UTF-16 caret mismatch",
        )?;
        require(target.window_id.is_some(), "Own window ID was not captured")?;
        self_editor_command(
            app,
            "main",
            json!({"op":"select", "kind":kind, "location":0}),
        )
        .await?;
        other_app.activate(app).await?;
        // Mirror a real utterance: leave the original editor while the actual
        // microphone remains active, with the AX helper idle for several seconds.
        tokio::time::sleep(Duration::from_secs(5)).await;
        voice_input::cancel_dictation(app.clone()).await?;
        let job = db::insert_paste_item("【自身】", PasteMode::Automatic, Some(&target), false)?;
        let delivered = super::process_ready(app, &job.id).await?;
        require(
            delivered.delivery_status == "verified",
            &format!("Own {kind} delivery: {:?}", delivered.error),
        )?;
        let content = self_editor_command(app, "main", json!({"op":"read", "kind":kind})).await?;
        require(
            content["text"] == "甲😀【自身】乙",
            &format!("Own {kind} text mismatch: {content}"),
        )?;
        results.push(
            json!({"case":format!("own_HVibeRec_{kind}_editor"), "target":target, "background_recording_ms":5000, "verified":true}),
        );
    }

    // A real editable field in our outbox must still be rejected, even though
    // it has the same PID and AX capabilities as the main editor.
    let panel = app
        .get_webview_window(super::WINDOW_LABEL)
        .ok_or("Missing tool window")?;
    panel.navigate(url).map_err(|e| e.to_string())?;
    panel.show().map_err(|e| e.to_string())?;
    panel.set_focus().map_err(|e| e.to_string())?;
    panel.as_ref().set_focus().map_err(|e| e.to_string())?;
    self_editor_command(
        app,
        super::WINDOW_LABEL,
        json!({"op":"select", "kind":"plain"}),
    )
    .await?;
    let error = super::capture(app)
        .await
        .err()
        .ok_or("Tool window was accepted as a target")?;
    require(
        error.contains("主窗口的编辑区"),
        &format!("Unexpected tool rejection: {error}"),
    )?;
    results.push(json!({"case":"own_tool_window_rejected", "verified":true}));
    Ok(())
}

async fn audit_shortcut_capture(
    app: &AppHandle,
    executable: &Path,
    results: &mut Vec<Value>,
) -> Result<(), String> {
    let mut original = Fixture::launch(executable, "HVR shortcut origin", false).await?;
    let mut other = Fixture::launch(executable, "HVR shortcut other", false).await?;
    original.activate(app).await?;
    db::set_pastebox_mode(PasteMode::Automatic)?;
    voice_input::apply_hotkey_registration(true, "Control+Option+Shift+K")?;
    let triggered = original.command(json!({"op":"voice_shortcut"})).await?;
    require(
        triggered["ok"] == true,
        "Could not send shortcut from owned fixture",
    )?;
    for _ in 0..100 {
        if voice_input::status().phase == "listening" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    require(
        voice_input::status().phase == "listening",
        "System shortcut did not start the microphone",
    )?;
    let shortcut_context =
        voice_input::audit_dictation_context().ok_or("System shortcut has no capture job")?;
    let (_, shortcut_target) = shortcut_context.resolve().await;
    let shortcut_target =
        shortcut_target.ok_or("System shortcut did not capture the original caret")?;
    require(
        shortcut_target.process_id == original.child.id().map(i64::from)
            && shortcut_target.selection_location == 3,
        "System shortcut captured the wrong origin",
    )?;
    voice_input::cancel_dictation(app.clone()).await?;
    results.push(json!({"case":"system_shortcut_records_owned_fixture", "target":shortcut_target, "verified":true}));
    // Use the production shortcut capture entry, then deliberately delay recorder
    // dispatch until after the user has moved the caret and changed applications.
    let context = super::begin_dictation_from_hotkey()?;
    let id = context.id().to_string();
    let (_, target) = context.clone().resolve().await;
    let target = target.ok_or("Shortcut origin capture failed")?;
    original
        .command(json!({"op":"select", "location":0}))
        .await?;
    other.activate(app).await?;
    voice_input::audit_start_with_capture(app.clone(), context).await?;
    let observed = voice_input::audit_dictation_context().ok_or("Shortcut context missing")?;
    require(
        observed.id() == id,
        "Recorder replaced the shortcut's capture job",
    )?;
    let (_, bound) = observed.resolve().await;
    require(
        bound.as_ref().map(|t| &t.id) == Some(&target.id),
        "Recorder retargeted after dispatch delay",
    )?;
    voice_input::cancel_dictation(app.clone()).await?;
    let item = db::insert_paste_item("【入口】", PasteMode::Automatic, bound.as_ref(), false)?;
    let delivered = super::process_ready(app, &item.id).await?;
    require(
        delivered.delivery_status == "verified",
        "Delayed shortcut target delivery failed",
    )?;
    require(
        original.command(json!({"op":"read"})).await?["text"] == "甲😀【入口】乙",
        "Delayed shortcut did not restore the original caret",
    )?;
    require(
        other.command(json!({"op":"read"})).await?["text"] == "甲😀乙",
        "Delayed shortcut wrote into the later foreground",
    )?;
    results.push(json!({"case":"shortcut_origin_survives_delayed_recorder_dispatch", "job":id, "target":target, "verified":true}));

    // Hold the capture response indefinitely. The real microphone must become
    // usable (and cancellable) before that response is released.
    let (release, waiting) = tokio::sync::oneshot::channel();
    let (finished, completion) = tokio::sync::oneshot::channel();
    let delayed = super::DictationContext::start(
        "audit-delayed-cancelled".into(),
        PasteMode::Automatic,
        async move {
            waiting.await.map_err(|e| e.to_string())?;
            let _ = finished.send(());
            Ok(target)
        },
    );
    let started = std::time::Instant::now();
    let status = tokio::time::timeout(
        Duration::from_secs(4),
        voice_input::audit_start_with_capture(app.clone(), delayed),
    )
    .await
    .map_err(|_| "Microphone waited for the capture response")??;
    require(
        status.phase == "listening" && status.message.contains("正在记录"),
        "Pending target blocked recording",
    )?;
    let startup_ms = started.elapsed().as_millis();
    voice_input::cancel_dictation(app.clone()).await?;
    other.activate(app).await?;
    voice_input::start_dictation(app.clone()).await?;
    let next = voice_input::audit_dictation_context().ok_or("Next recording context missing")?;
    let next_id = next.id().to_string();
    release
        .send(())
        .map_err(|_| "Capture task was aborted on cancellation")?;
    completion.await.map_err(|e| e.to_string())?;
    let (_, next_target) = next.resolve().await;
    require(
        next_target.as_ref().and_then(|t| t.process_id) == other.child.id().map(i64::from),
        "Cancelled capture supplied the next recording's target",
    )?;
    require(
        voice_input::audit_dictation_context()
            .as_ref()
            .map(|c| c.id())
            == Some(next_id.as_str()),
        "Late capture replaced the active recording",
    )?;
    voice_input::cancel_dictation(app.clone()).await?;
    results.push(json!({"case":"pending_capture_allows_recording_cancel_and_next_job", "microphone_startup_ms":startup_ms, "verified":true}));
    Ok(())
}

async fn audit_unverified_navigation(
    app: &AppHandle,
    executable: &Path,
    other: &mut Fixture,
    results: &mut Vec<Value>,
) -> Result<(), String> {
    let mut target = Fixture::launch(
        executable,
        "HVR audit accepted paste with normalization",
        false,
    )
    .await?;
    let captured = record_target(app, &mut target, PasteMode::Automatic).await?;
    target.command(json!({"op":"decorate_next_paste"})).await?;
    let item = db::insert_paste_item("【已写入】", PasteMode::Automatic, Some(&captured), false)?;
    let delivered = super::process_ready(app, &item.id).await?;
    require(
        delivered.delivery_status == "sent",
        "Fixture did not reproduce unverified receipt",
    )?;
    let accepted = target.command(json!({"op":"read"})).await?["text"].clone();
    require(
        accepted == "甲😀【已写入】乙\nrendered",
        "Fixture did not actually accept the paste",
    )?;
    target.command(json!({"op":"select","location":0})).await?;
    other.activate(app).await?;
    for _ in 0..2 {
        super::notifications::clicked(app, &item.id).await?;
        let front = super::native::call(app, json!({"op":"frontmost"})).await?;
        require(
            front["pid"].as_u64() == target.child.id().map(u64::from),
            "Notification did not return to original app",
        )?;
        require(
            !app.get_webview_window(super::WINDOW_LABEL)
                .unwrap()
                .is_visible()
                .unwrap(),
            "Notification stole focus into pastebox",
        )?;
        require(
            target.command(json!({"op":"read"})).await?["text"] == accepted,
            "Notification repeated an accepted paste",
        )?;
    }
    let saved = db::get_paste_item(&item.id)?;
    require(
        saved.delivery_status == "sent"
            && saved
                .notification_error
                .as_deref()
                .unwrap_or("")
                .contains("光标位置未确认"),
        "Window-only navigation must be explicit without upgrading delivery status",
    )?;
    require(
        target.command(json!({"op":"selection"})).await?["location"] == 0,
        "Unconfirmed navigation guessed a new caret",
    )?;
    results.push(json!({"case":"accepted_paste_unverified_receipt_notification_returns_without_panel_or_input", "verified":true}));

    target.command(json!({"op":"close"})).await?;
    other.activate(app).await?;
    require(
        super::notifications::clicked(app, &item.id).await.is_err(),
        "Closed window incorrectly reported restored",
    )?;
    require(
        !app.get_webview_window(super::WINDOW_LABEL)
            .unwrap()
            .is_visible()
            .unwrap(),
        "Closed-window notification opened pastebox",
    )?;
    let front = super::native::call(app, json!({"op":"frontmost"})).await?;
    // AX recovery may activate the original app before discovering that its
    // retained NSWindow was closed. Feedback must not activate H-VibeRec.
    require(
        front["pid"].as_u64() != Some(u64::from(std::process::id())),
        "Failure feedback activated H-VibeRec",
    )?;
    require(
        db::get_paste_item(&item.id)?.notification_error.is_some(),
        "Navigation error was not retained",
    )?;
    results.push(json!({"case":"closed_notification_target_reports_error_without_panel_or_input", "verified":true}));
    let semi = db::insert_paste_item("不得重试", PasteMode::Notify, Some(&captured), false)?;
    require(
        super::notifications::clicked(app, &semi.id).await.is_err(),
        "Closed semi-automatic target was not rejected",
    )?;
    require(
        !app.get_webview_window(super::WINDOW_LABEL)
            .unwrap()
            .is_visible()
            .unwrap(),
        "Failed semi-automatic click opened pastebox",
    )?;
    require(
        db::get_paste_item(&semi.id)?.delivery_status == "failed",
        "Failed semi-automatic item was lost",
    )?;
    results.push(json!({"case":"semi_automatic_failure_retains_item_without_opening_panel", "verified":true}));
    Ok(())
}

async fn wait_item(
    id: &str,
    predicate: impl Fn(&crate::types::PasteItem) -> bool,
) -> Result<crate::types::PasteItem, String> {
    for _ in 0..160 {
        let item = db::get_paste_item(id)?;
        if predicate(&item) {
            return Ok(item);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(format!("Timed out waiting for background job {id}"))
}

fn audit_refinement(mode: &str) -> Result<(), String> {
    db::connect()?
        .execute(
            "INSERT OR REPLACE INTO app_settings (key,value,updated_at)
        VALUES ('voice_input_refinement_mode',?1,?2)",
            [mode.to_string(), db::now_iso()],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn audit_background_jobs(
    app: &AppHandle,
    executable: &Path,
    results: &mut Vec<Value>,
) -> Result<(), String> {
    use voice_input::jobs::{audit_block, paths, retry};
    let mut a = Fixture::launch(executable, "HVR async A", false).await?;
    let mut b = Fixture::launch(executable, "HVR async B", false).await?;
    let mut c = Fixture::launch(executable, "HVR async active recording", false).await?;
    audit_refinement("ai_polish")?;
    db::set_pastebox_mode(PasteMode::Automatic)?;
    a.activate(app).await?;
    voice_input::start_dictation(app.clone()).await?;
    let a_context = voice_input::audit_dictation_context().ok_or("A context missing")?;
    let asr_a = audit_block(a_context.id(), "asr");
    let polish_a = audit_block(a_context.id(), "polish");
    tokio::time::sleep(Duration::from_millis(900)).await;
    let stop_at = std::time::Instant::now();
    let submitted_a = voice_input::stop_dictation(app.clone()).await?;
    let stop_ms = stop_at.elapsed().as_millis();
    require(stop_ms < 1500, "Stop waited for blocked transcription")?;
    require(
        voice_input::status().phase == "idle",
        "Submitted recording did not release microphone",
    )?;
    require(
        voice_input::stop_dictation(app.clone()).await.is_err(),
        "Duplicate stop submitted another job",
    )?;
    a.command(json!({"op":"select","location":0})).await?;
    a.command(json!({"op":"move"})).await?;
    b.activate(app).await?;
    voice_input::start_dictation(app.clone()).await?;
    let b_context = voice_input::audit_dictation_context().ok_or("B context missing")?;
    let asr_b = audit_block(b_context.id(), "asr");
    let polish_b = audit_block(b_context.id(), "polish");
    require(
        db::get_paste_item(&submitted_a.job_id)?.raw_text.is_empty(),
        "A was not blocked",
    )?;
    // Changes while B is recording apply only to C.
    audit_refinement("raw")?;
    db::set_pastebox_mode(PasteMode::Manual)?;
    tokio::time::sleep(Duration::from_millis(900)).await;
    let submitted_b = voice_input::stop_dictation(app.clone()).await?;
    require(
        submitted_a.seq < submitted_b.seq,
        "Sequence is not submission order",
    )?;
    results.push(json!({"case":"stop_returns_while_asr_blocked_and_next_recording_submits", "stop_ms":stop_ms, "verified":true}));

    c.activate(app).await?;
    voice_input::start_dictation(app.clone()).await?;
    let c_id = voice_input::audit_dictation_context()
        .unwrap()
        .id()
        .to_string();
    asr_a
        .send(Ok("原文 A".into()))
        .map_err(|_| "A ASR hook dropped")?;
    asr_b
        .send(Ok("原文 B".into()))
        .map_err(|_| "B ASR hook dropped")?;
    polish_b
        .send(Ok("【异步B】".into()))
        .map_err(|_| "B polish hook dropped")?;
    let ready_b = wait_item(&submitted_b.job_id, |i| i.processing_status == "ready").await?;
    require(
        ready_b.text == "【异步B】" && ready_b.mode == PasteMode::Automatic,
        "B lost recording-start settings/mode snapshot",
    )?;
    require(
        db::get_paste_item(&submitted_a.job_id)?.processing_status == "refining",
        "B ASR waited for A polish",
    )?;
    require(ready_b.delivery_status == "pending", "B overtook earlier A")?;
    results.push(json!({"case":"later_asr_and_polish_complete_before_earlier_polish_with_frozen_settings", "verified":true}));
    polish_a
        .send(Ok("【异步A】".into()))
        .map_err(|_| "A polish hook dropped")?;
    wait_item(&submitted_a.job_id, |i| i.processing_status == "ready").await?;
    tokio::time::sleep(Duration::from_millis(800)).await;
    require(
        voice_input::status().phase == "listening"
            && voice_input::audit_dictation_context().unwrap().id() == c_id,
        "Older job completion changed C recorder state",
    )?;
    require(
        a.command(json!({"op":"read"})).await?["text"] == "甲😀乙",
        "Automatic delivery ran during C recording",
    )?;
    require(
        b.command(json!({"op":"read"})).await?["text"] == "甲😀乙",
        "B was pasted during C recording",
    )?;
    results.push(json!({"case":"background_completion_preserves_active_recorder_and_defers_focus", "verified":true}));
    voice_input::cancel_dictation(app.clone()).await?;
    wait_item(&submitted_b.job_id, |i| i.delivery_status == "verified").await?;
    require(
        db::get_paste_item(&submitted_a.job_id)?.delivery_status == "verified",
        "Automatic B passed A",
    )?;
    require(
        a.command(json!({"op":"read"})).await?["text"] == "甲😀【异步A】乙",
        "A not written at original caret",
    )?;
    require(
        b.command(json!({"op":"read"})).await?["text"] == "甲😀【异步B】乙",
        "B not written at original caret",
    )?;
    results.push(json!({"case":"cancel_current_recording_resumes_fifo_delivery_to_two_original_windows", "verified":true}));

    c.activate(app).await?;
    voice_input::start_dictation(app.clone()).await?;
    let failure_id = voice_input::audit_dictation_context()
        .unwrap()
        .id()
        .to_string();
    let failed_asr = audit_block(&failure_id, "asr");
    tokio::time::sleep(Duration::from_millis(900)).await;
    let failed_submission = voice_input::stop_dictation(app.clone()).await?;
    voice_input::start_dictation(app.clone()).await?;
    failed_asr
        .send(Err("deterministic ASR failure".into()))
        .map_err(|_| "failure hook dropped")?;
    let failed = wait_item(&failure_id, |i| i.processing_status == "failed").await?;
    require(
        paths(&failure_id)?.0.is_file(),
        "ASR failure deleted retained audio",
    )?;
    require(
        voice_input::status().phase == "listening",
        "A failed ASR reset next recorder",
    )?;
    voice_input::cancel_dictation(app.clone()).await?;
    let retried_asr = audit_block(&failure_id, "asr");
    retry(app.clone(), &failure_id, failed.version).await?;
    retried_asr
        .send(Ok("重试原文".into()))
        .map_err(|_| "retry hook dropped")?;
    let retried = wait_item(&failure_id, |i| i.processing_status == "ready").await?;
    require(
        retried.seq == failed_submission.seq && retried.mode == PasteMode::Manual,
        "Retry changed identity/mode",
    )?;
    require(
        !paths(&failure_id)?.0.is_file(),
        "Successful ASR left temporary audio",
    )?;
    require(
        c.command(json!({"op":"read"})).await?["text"] == "甲😀乙",
        "Manual mode pasted automatically",
    )?;
    results.push(json!({"case":"asr_failure_keeps_audio_and_recorder_then_retries_same_manual_job", "verified":true}));

    db::set_pastebox_mode(PasteMode::Notify)?;
    voice_input::start_dictation(app.clone()).await?;
    let notify_id = voice_input::audit_dictation_context()
        .unwrap()
        .id()
        .to_string();
    let notify_asr = audit_block(&notify_id, "asr");
    tokio::time::sleep(Duration::from_millis(900)).await;
    voice_input::stop_dictation(app.clone()).await?;
    a.activate(app).await?;
    voice_input::start_dictation(app.clone()).await?;
    notify_asr
        .send(Ok("【通知异步】".into()))
        .map_err(|_| "notify hook dropped")?;
    wait_item(&notify_id, |i| i.processing_status == "ready").await?;
    super::handle_native_event(json!({"type":"notification", "id":notify_id}));
    tokio::time::sleep(Duration::from_millis(300)).await;
    require(
        voice_input::status().phase == "listening",
        "Notification changed recorder",
    )?;
    require(
        c.command(json!({"op":"read"})).await?["text"] == "甲😀乙",
        "Notification stole focus during recording",
    )?;
    voice_input::cancel_dictation(app.clone()).await?;
    wait_item(&notify_id, |i| i.delivery_status == "verified").await?;
    require(
        c.command(json!({"op":"read"})).await?["text"] == "甲😀【通知异步】乙",
        "Notification targeted wrong async job",
    )?;
    results.push(json!({"case":"semi_automatic_callback_waits_for_recorder_then_pastes_its_exact_job", "verified":true}));
    Ok(())
}

async fn run(app: &AppHandle, executable: &Path, results: &mut Vec<Value>) -> Result<(), String> {
    require(
        db::list_paste_items(None, "", false)?.is_empty(),
        "Audit requires an empty database",
    )?;
    // This suite tests delivery/callbacks in isolated fixtures, without sending
    // system banners to the user's Notification Center.
    super::set_notifications_enabled(app, false).await?;
    db::connect()?
        .execute(
            "INSERT OR REPLACE INTO app_settings (key,value,updated_at) VALUES ('voice_input_enabled','true',?1)",
            [db::now_iso()],
        )
        .map_err(|e| e.to_string())?;
    for window in app.webview_windows().values() {
        let _ = window.hide();
    }
    require(
        super::accessibility_status(app).await?,
        "AX permission missing for audit helper",
    )?;
    let token = uuid::Uuid::new_v4().to_string();
    let mut a = Fixture::launch(executable, &format!("HVR audit {token} A"), false).await?;
    if std::env::var("HVR_AX_AUDIT_ONLY_SELF").as_deref() == Ok("1") {
        // Finish the fixture's initial app activation before activating our main
        // window, so its launch cannot later race the own-editor precondition.
        a.activate(app).await?;
        return audit_own_editors(app, &mut a, results).await;
    }
    let target = record_target(app, &mut a, PasteMode::Automatic).await?;
    require(
        target.selection_location == 3 && target.selection_length == 0,
        "Wrong initial UTF-16 caret",
    )?;
    a.command(json!({"op":"select","location":0})).await?;
    a.command(json!({"op":"move"})).await?;
    let mut b = Fixture::launch(executable, &format!("HVR audit {token} B"), false).await?;
    b.activate(app).await?;
    let job = db::insert_paste_item("【录音目标】", PasteMode::Automatic, Some(&target), false)?;
    let delivered = super::process_ready(app, &job.id).await?;
    require(
        delivered.delivery_status == "verified",
        &format!("Automatic: {:?}", delivered.error),
    )?;
    require(
        a.command(json!({"op":"read"})).await?["text"] == "甲😀【录音目标】乙",
        "Original caret not restored",
    )?;
    results.push(
        json!({"case":"real_recorder_then_switch_and_move", "target":target,"verified":true}),
    );

    require(
        delivered.notification_error.is_none(),
        "Disabled notification attempted enqueue",
    )?;
    a.command(json!({"op":"select","location":0})).await?;
    b.activate(app).await?;
    super::notifications::clicked(app, &job.id).await?;
    require(
        a.command(json!({"op":"selection"})).await?["location"] == 9,
        "Automatic notification did not restore its insertion end",
    )?;
    super::notifications::clicked(app, &job.id).await?;
    require(
        a.command(json!({"op":"read"})).await?["text"] == "甲😀【录音目标】乙",
        "Automatic notification pasted a second time",
    )?;
    results
        .push(json!({"case":"automatic_notification_restores_without_repaste", "verified":true}));

    // A second automatic item in the same input must not move the first item's
    // return position; IDs can be clicked in either order after switching apps.
    let auto_target = record_target(app, &mut b, PasteMode::Automatic).await?;
    let auto_a = db::insert_paste_item("一", PasteMode::Automatic, Some(&auto_target), false)?;
    super::process_ready(app, &auto_a.id).await?;
    b.command(json!({"op":"select","location":4})).await?;
    let auto_target_b = record_target(app, &mut b, PasteMode::Automatic).await?;
    let auto_b = db::insert_paste_item("二", PasteMode::Automatic, Some(&auto_target_b), false)?;
    super::process_ready(app, &auto_b.id).await?;
    a.activate(app).await?;
    super::notifications::clicked(app, &auto_b.id).await?;
    require(
        b.command(json!({"op":"selection"})).await?["location"] == 5,
        "Wrong automatic item B caret",
    )?;
    super::notifications::clicked(app, &auto_a.id).await?;
    require(
        b.command(json!({"op":"selection"})).await?["location"] == 4,
        "Wrong automatic item A caret",
    )?;
    require(
        b.command(json!({"op":"read"})).await?["text"] == "甲😀一二乙",
        "Automatic reverse click changed text",
    )?;
    results.push(
        json!({"case":"automatic_notifications_reverse_ID_order_same_input", "verified":true}),
    );
    // Leave B at its original start for the semi-automatic checks below.
    b.command(json!({"op":"select","location":3})).await?;

    a.command(json!({"op":"select","location":1,"length":2}))
        .await?;
    let ta = record_target(app, &mut a, PasteMode::Notify).await?;
    let ja = db::insert_paste_item("【A】", PasteMode::Notify, Some(&ta), false)?;
    let tb = record_target(app, &mut b, PasteMode::Notify).await?;
    let jb = db::insert_paste_item("【B】", PasteMode::Notify, Some(&tb), false)?;
    let quiet = super::process_ready(app, &jb.id).await?;
    require(
        quiet.delivery_status == "pending" && quiet.notification_error.is_none(),
        "Disabled semi-automatic notifications must leave a pending queue item",
    )?;
    a.command(json!({"op":"minimize"})).await?;
    notify_and_wait(&jb.id).await?;
    notify_and_wait(&ja.id).await?;
    notify_and_wait(&ja.id).await?;
    tokio::time::sleep(Duration::from_millis(150)).await;
    require(
        a.command(json!({"op":"read"})).await?["text"] == "甲【A】【录音目标】乙",
        "Selection/duplicate notification mismatch",
    )?;
    require(
        b.command(json!({"op":"read"})).await?["text"] == "甲😀【B】一二乙",
        "Notification B went to wrong target",
    )?;
    results.push(
        json!({"case":"notification_B_then_A_and_duplicate_minimized_window", "verified":true}),
    );

    a.command(json!({"op":"select","location":0})).await?;
    let tm = record_target(app, &mut a, PasteMode::Manual).await?;
    a.command(json!({"op":"other_window", "same_title":true}))
        .await?;
    let jm = db::insert_paste_item("【同应用】", PasteMode::Manual, Some(&tm), false)?;
    let queued = super::process_ready(app, &jm.id).await?;
    require(
        queued.delivery_status == "pending",
        "Manual mode delivered automatically",
    )?;
    require(
        a.command(json!({"op":"read"})).await?["text"] == "甲【A】【录音目标】乙",
        "Manual queue changed the original document",
    )?;
    let delivered = super::deliver_pending(app, &jm.id).await?;
    require(
        delivered.delivery_status == "verified",
        &format!("Same-process window: {:?}", delivered.error),
    )?;
    require(
        a.command(json!({"op":"read"})).await?["text"] == "【同应用】甲【A】【录音目标】乙",
        "Original window/caret in same process was not restored",
    )?;
    require(
        a.command(json!({"op":"read_other"})).await?["text"] == "另一个窗口",
        "Delivery went to the other window of the same application",
    )?;
    results.push(json!({"case":"manual_queue_then_exact_window_in_same_process", "verified":true}));

    let mut web = Fixture::launch(executable, &format!("HVR audit {token} WebKit"), true).await?;
    let tw = record_target(app, &mut web, PasteMode::Automatic).await?;
    a.activate(app).await?;
    let jw = db::insert_paste_item("【网页】", PasteMode::Automatic, Some(&tw), false)?;
    let delivered = super::process_ready(app, &jw.id).await?;
    require(
        delivered.delivery_status == "verified",
        &format!("WebKit: {:?}", delivered.error),
    )?;
    require(
        web.command(json!({"op":"read"})).await?["text"] == "甲😀【网页】乙",
        "WebKit caret mismatch",
    )?;
    results.push(json!({"case":"real_recorder_WebKit_textarea", "target":tw,"verified":true}));

    web.command(json!({"op":"close"})).await?;
    a.activate(app).await?;
    let closed = db::insert_paste_item("不得写入", PasteMode::Automatic, Some(&tw), false)?;
    let rejected = super::process_ready(app, &closed.id).await?;
    require(
        rejected.delivery_status == "failed",
        "Closed target was not rejected",
    )?;
    require(
        a.command(json!({"op":"read"})).await?["text"] == "【同应用】甲【A】【录音目标】乙",
        "Closed-target delivery leaked to foreground",
    )?;
    results.push(json!({"case":"closed_window_retains_queue_without_input", "verified":true}));
    super::close(app)?;
    audit_unverified_navigation(app, executable, &mut a, results).await?;
    audit_own_editors(app, &mut a, results).await?;
    audit_shortcut_capture(app, executable, results).await?;
    audit_background_jobs(app, executable, results).await?;
    Ok(())
}

pub(crate) fn schedule(app: AppHandle) {
    let Some(fixture) = std::env::var_os("HVR_AX_AUDIT_FIXTURE") else {
        return;
    };
    let Some(root) = std::env::var_os("VOICE_VIBE_TEST_APP_DATA_DIR") else {
        return;
    };
    let fixture = std::path::PathBuf::from(fixture);
    let Ok(root) = std::path::PathBuf::from(root).canonicalize() else {
        return;
    };
    if fixture.file_name().and_then(|s| s.to_str()) != Some("AXFixture")
        || root.parent() != Some(Path::new("/private/tmp"))
        || !root
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("h-viberec-target-qa."))
    {
        log::error!("AX audit requires the dedicated temporary fixture/data directory");
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(750)).await;
        let mut cases = Vec::new();
        let result = run(&app, &fixture, &mut cases).await;
        let report = json!({"passed":result.is_ok(), "cases":cases, "error":result.err(),
            "boundary":"real recorder + Tauri outbox + AX; injected transcript; notification callback, not OS banner"});
        let _ = voice_input::cancel_dictation(app.clone()).await;
        let _ = std::fs::write(
            root.join("target-audit.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        );
        log::info!("AX recorder audit: {report}");
        app.exit(if report["passed"] == true { 0 } else { 1 });
    });
}
