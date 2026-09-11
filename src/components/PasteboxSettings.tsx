import { useEffect, useState } from 'react';
import { Bell, Clipboard } from 'lucide-react';
import { pasteboxService } from '../services/pastebox.service';
import { PASTE_MODES } from '../lib/pastebox';
import type { PasteboxState } from '../types/pastebox';
import './PasteboxPanel.css';

export default function PasteboxSettings() {
  const [state, setState] = useState<PasteboxState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const refresh = () => void pasteboxService.getState().then(value => { if (!disposed) setState(value); }).catch(cause => { if (!disposed) setError(String(cause)); });
    void pasteboxService.onChange(refresh).then(cleanup => { if (disposed) cleanup(); else unlisten = cleanup; })
      .catch(cause => { if (!disposed) setError(String(cause)); });
    refresh();
    return () => { disposed = true; unlisten?.(); };
  }, []);
  async function run(action: () => Promise<void>) {
    setBusy(true); setError(null);
    try { await action(); setState(await pasteboxService.getState()); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }
  return <section className="pastebox-settings" aria-label="菜单栏粘贴箱配置">
    <strong>菜单栏粘贴箱</strong>
    <p>所有转录和润色结果保存在本机。以下投递模式立即生效，影响后续听写。</p>
    <div className="pastebox-settings-actions">
      {PASTE_MODES.map(mode => <button key={mode.value} className={state?.mode === mode.value ? 'btn-primary' : 'btn-secondary'}
        aria-pressed={state?.mode === mode.value} disabled={busy || !state} onClick={() => void run(() => pasteboxService.setMode(mode.value))}>{mode.label}</button>)}
      <button className="btn-secondary" onClick={() => void run(() => pasteboxService.open())}><Clipboard size={14} />打开粘贴箱</button>
      {state?.notifications_enabled && state.notification_status !== 'granted' && <button className="btn-secondary" disabled={busy} onClick={() => void run(() => pasteboxService.requestNotifications())}><Bell size={14} />启用系统通知</button>}
      {state && !state.accessibility_trusted && <button className="btn-secondary" disabled={busy} onClick={() => void run(() => pasteboxService.requestAccessibility())}>启用辅助功能</button>}
    </div>
    <p>{PASTE_MODES.find(mode => mode.value === state?.mode)?.description}</p>
    <label className="pastebox-notification-toggle">
      <input type="checkbox" checked={state?.notifications_enabled ?? true} disabled={busy || !state}
        onChange={event => void run(() => pasteboxService.setNotificationsEnabled(event.target.checked))} />
      完成通知（自动 / 半自动）
    </label>
    <p>{state?.notifications_enabled === false ? '通知已关闭。自动模式仍自动粘贴；半自动模式请从顶部栏粘贴箱点击处理。' : '自动模式：点击通知返回对应位置。半自动模式：点击通知返回对应位置并粘贴，每条通知对应自己的记录。'}</p>
    {state?.notifications_enabled && state.notification_status === 'denied' && <p role="status">请在 macOS 系统设置 → 通知中允许 H-VibeRec 发送通知。</p>}
    <p>每次听写自动记录开始位置，无需额外快捷键。点击 Mac 顶部栏图标即可打开粘贴箱。</p>
    {(error || state?.backend_error) && <p role="alert">{error || state?.backend_error}</p>}
  </section>;
}
