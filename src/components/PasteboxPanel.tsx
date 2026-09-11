import { useEffect, useState } from 'react';
import { ArrowDown, ArrowUpRight, Bell, Check, Clipboard, Copy, Loader2, RotateCcw, Search, Trash2, X } from 'lucide-react';
import { usePastebox } from '../hooks/usePastebox';
import { PASTE_MODES, pasteStatus, resolvePasteTarget } from '../lib/pastebox';
import type { PasteItem } from '../types/pastebox';
import './PasteboxPanel.css';

export default function PasteboxPanel() {
  const box = usePastebox();
  const [rawIds, setRawIds] = useState<Set<string>>(new Set());
  const [deleteId, setDeleteId] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const { state, service, run } = box;

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.preventDefault(); void service.close(); }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [service]);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 3500);
    return () => window.clearTimeout(timer);
  }, [notice]);

  function act(item: PasteItem, copyOnly: boolean) {
    void run(async () => {
      if (!state) return;
      const targetId = copyOnly ? undefined : resolvePasteTarget(item, state, box.targetChoice);
      const result = await service.act({ id: item.id, version: item.version, targetId,
        raw: rawIds.has(item.id) || item.processing_status === 'refining', copyOnly });
      if (result.delivery_status === 'failed') throw new Error(result.error || '粘贴失败，内容已保留');
      setNotice(copyOnly ? '已复制，可以自行粘贴' : '已发送到目标位置');
    });
  }

  const opening = state?.targets.find(target => target.id === state.opening_target_id);
  const visibleItems = state?.requested_item && !box.query && !box.pendingOnly && !box.items.some(item => item.id === state.requested_item_id)
    ? [state.requested_item, ...box.items] : box.items;
  return (
    <main className="pastebox-panel">
      <header className="pastebox-header">
        <span className="pastebox-mark"><Clipboard size={19} /></span>
        <div><h1>语音粘贴箱</h1><p>录音结束即可继续，后台按段处理</p></div>
        <span className="pastebox-count">{state?.pending_count || 0} 待处理</span>
        <button className="pastebox-icon" aria-label="关闭粘贴箱" onClick={() => void service.close()}><X size={16} /></button>
      </header>

      <section className="pastebox-controls" aria-label="粘贴设置">
        <div className="pastebox-modes" role="group" aria-label="投递模式">
          {PASTE_MODES.map(mode => (
            <button key={mode.value} aria-pressed={state?.mode === mode.value} disabled={box.busy || !state}
              onClick={() => void run(() => service.setMode(mode.value))}>{mode.label}</button>
          ))}
        </div>
        <p className="pastebox-mode-hint">{PASTE_MODES.find(mode => mode.value === state?.mode)?.description || '正在读取本地历史…'}</p>
        <label className="pastebox-target"><span>点击条目粘贴到</span>
          <select aria-label="本次粘贴位置" value={box.targetChoice} disabled={box.busy} onChange={event => box.setTargetChoice(event.target.value)}>
            <option value="opening">{opening ? `打开前的位置 · ${opening.app_name}` : '打开前的位置 · 未记录'}</option>
            <option value="original">每条听写的原位置</option>
            {state?.targets.map(target => <option key={target.id} value={target.id} disabled={!target.available}>
              {target.app_name} · {target.window_title || '未命名窗口'} · {new Date(target.captured_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}{target.availability === 'unreachable' ? '（暂时不可访问）' : target.available ? '' : '（已失效）'}
            </option>)}
          </select>
        </label>
        <label className="pastebox-notification-toggle">
          <input type="checkbox" checked={state?.notifications_enabled ?? true} disabled={box.busy || !state}
            onChange={event => void run(() => service.setNotificationsEnabled(event.target.checked))} />
          完成通知（自动 / 半自动）
        </label>
        {state?.notifications_enabled === false && <p className="pastebox-mode-hint">自动模式仍自动粘贴；半自动模式请在这里点击处理。</p>}
      </section>

      {state?.mode !== 'manual' && state?.notifications_enabled && state.notification_status !== 'granted' && (
        <div className="pastebox-notification-note"><Bell size={16} /><div>
          <span>{state.notification_status === 'denied' ? '通知被关闭，请在 macOS 系统设置中允许 H-VibeRec 通知。' :
            state.notification_status === 'unavailable' ? '系统通知需从 H-VibeRec.app 启用；当前内容仍会保存。' : '启用通知后，自动模式点击返回位置，半自动模式点击粘贴。'}</span>
          {state.notification_status === 'not_determined' && <button disabled={box.busy} onClick={() => void run(() => service.requestNotifications())}>启用系统通知</button>}
        </div></div>
      )}
      {state && !state.accessibility_trusted && <div className="pastebox-notification-note"><div>
        <span>记录和恢复输入位置需要辅助功能权限。授权后请重新打开粘贴箱。</span>
        <button disabled={box.busy} onClick={() => void run(() => service.requestAccessibility())}>启用辅助功能</button>
      </div></div>}
      {(box.error || state?.backend_error || state?.target_error) && <div className="pastebox-error" role="alert">{box.error || state?.backend_error || state?.target_error}</div>}
      {notice && <div className="pastebox-notice" role="status"><Check size={14} />{notice}</div>}

      <div className="pastebox-history-toolbar">
        <div className="pastebox-filters" role="group" aria-label="历史筛选">
          <button aria-pressed={box.pendingOnly} disabled={box.busy} onClick={() => box.setPendingOnly(true)}>待处理</button>
          <button aria-pressed={!box.pendingOnly} disabled={box.busy} onClick={() => box.setPendingOnly(false)}>全部历史</button>
        </div>
        <button className="pastebox-next" title="按编号从小到大，粘贴下一条到它记录的原位置" disabled={box.busy || !state?.pending_count}
          onClick={() => void run(async () => {
            const item = await service.pasteNext();
            if (!item) setNotice('没有已就绪的未处理内容');
            else if (item.delivery_status === 'failed') throw new Error(item.error || '原位置不可用');
          })}><ArrowDown size={13} />按顺序处理</button>
        <button className="pastebox-clear-history" title="清空已完成和失败的历史记录，正在处理的任务会保留" disabled={box.busy || !box.items.some(item => ['ready', 'failed'].includes(item.processing_status))}
          onClick={() => void run(async () => {
            const count = await service.clearHistory();
            setNotice(count ? `已清空 ${count} 条历史记录` : '没有可清空的历史记录');
          })}><Trash2 size={13} />一键清空</button>
      </div>
      <label className="pastebox-search"><Search size={14} /><input aria-label="搜索转录或编号" placeholder="搜索内容或编号" value={box.query} disabled={box.busy}
        onChange={event => box.setQuery(event.target.value)} /></label>

      <section className="pastebox-items" aria-label="转录历史" aria-busy={box.loading}>
        {box.loading && <div className="pastebox-empty"><Loader2 size={22} className="spinning" /><p>正在读取粘贴箱</p></div>}
        {!box.loading && visibleItems.length === 0 && <div className="pastebox-empty"><Clipboard size={30} />
          <h2>{box.query ? '没有匹配的内容' : box.pendingOnly ? '暂时没有待处理内容' : '从一次语音输入开始'}</h2>
          <p>{box.query ? '试试更短的关键词或记录编号。' : '转录与润色结果会保存在这里，点击即可再次使用。'}</p>
        </div>}
        {visibleItems.map(item => {
          const raw = rawIds.has(item.id) || item.processing_status === 'refining';
          const disabled = box.busy || item.delivery_status === 'delivering';
          const hasText = !!item.raw_text.trim();
          const processing = !['ready', 'failed'].includes(item.processing_status);
          const text = raw ? item.raw_text : item.text;
          return <article key={item.id} className={`pastebox-item ${state?.requested_item_id === item.id ? 'requested' : ''}`}>
            <div className="pastebox-item-meta"><span className="pastebox-seq" title={`记录 ID：${item.id}`}>#{item.seq}</span>
              <time dateTime={item.created_at}>{new Date(item.created_at).toLocaleString([], { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })}</time>
              <span className={`pastebox-status ${item.delivery_status}`}>{pasteStatus(item)}</span>
            </div>
            <button className="pastebox-text" title="点击粘贴到上方选中的位置" disabled={disabled || !hasText} onClick={() => act(item, false)}>
              <span>{text || (item.processing_status === 'failed' ? '录音已保留，可重试转录。' : '已收到录音，正在后台处理，可继续录制下一段。')}</span>{hasText && <ArrowUpRight size={15} />}
            </button>
            <div className="pastebox-item-target" title={item.target?.unavailable_reason || item.target?.window_title}>原位置：{item.target?.app_name || '未记录'}
              {item.target?.window_title ? ` · ${item.target.window_title}` : ''}
              {item.target?.availability === 'unreachable' ? ` · 暂时不可访问${item.target.available ? '，点击可重试恢复' : ''}` :
                item.target && !item.target.available ? ' · 已失效，可选择新位置' : ''}</div>
            {item.error && <p className="pastebox-item-warning">{item.error}</p>}
            {item.polish_error && <p className="pastebox-item-warning">润色未完成：{item.polish_error}</p>}
            {item.notification_error && <p className="pastebox-item-warning">{item.notification_error}</p>}
            <div className="pastebox-item-actions">
              <div className="pastebox-version" role="group" aria-label={`第 ${item.seq} 条文本版本`}>
                <button aria-pressed={raw} disabled={!hasText} onClick={() => setRawIds(current => new Set(current).add(item.id))}>原文</button>
                <button aria-pressed={!raw} disabled={processing || !hasText} onClick={() => setRawIds(current => { const next = new Set(current); next.delete(item.id); return next; })}>最终文本</button>
              </div>
              {item.processing_status === 'failed' && !hasText && <button disabled={disabled}
                title="使用当前转录设置重试，保留本条原位置和模式" onClick={() => void run(() => service.retry(item.id, item.version))}><RotateCcw size={13} />重试</button>}
              <button disabled={disabled || !hasText} onClick={() => act(item, true)}><Copy size={13} />仅复制</button>
              <button className="pastebox-delete" disabled={disabled || processing} aria-label={`删除第 ${item.seq} 条`}
                onClick={() => { if (deleteId === item.id) void run(async () => { await service.delete(item.id, item.version); setDeleteId(null); }); else setDeleteId(item.id); }}>
                {deleteId === item.id ? '确认删除' : <Trash2 size={13} />}
              </button>
            </div>
            <details className="pastebox-full"><summary>展开全文</summary><pre>{text}</pre><span>记录 ID：{item.id}</span></details>
          </article>;
        })}
        {box.hasMore && <button className="pastebox-load-more" disabled={box.busy} onClick={() => void box.refresh(box.items[box.items.length - 1]?.seq)}>加载更早的内容</button>}
      </section>
      <footer className="pastebox-footer">
        <span title={state?.target_error || undefined}>{state?.target_error ? '位置未就绪 · 可仅复制' : '内容保存在本机'}</span>
        <button onClick={() => void run(() => service.openMain())}>打开主窗口</button>
        <button onClick={() => void run(() => service.quit())}>退出</button>
      </footer>
    </main>
  );
}
