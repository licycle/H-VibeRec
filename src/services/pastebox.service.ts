import type { PasteAction, PasteItem, PasteMode, PasteboxQuery, PasteboxState } from '../types/pastebox';
import { TauriPasteboxClient } from '../api/pastebox.client';

export interface PasteboxService {
  getState(query?: PasteboxQuery): Promise<PasteboxState>;
  open(): Promise<void>;
  close(): Promise<void>;
  setMode(mode: PasteMode): Promise<void>;
  setNotificationsEnabled(enabled: boolean): Promise<void>;
  act(action: PasteAction): Promise<PasteItem>;
  pasteNext(): Promise<PasteItem | null>;
  retry(id: string, version: number): Promise<void>;
  delete(id: string, version: number): Promise<void>;
  clearHistory(): Promise<number>;
  requestNotifications(): Promise<void>;
  requestAccessibility(): Promise<void>;
  openMain(): Promise<void>;
  quit(): Promise<void>;
  onChange(listener: () => void): Promise<() => void>;
  onOpen(listener: () => void): Promise<() => void>;
}

// The browser can preview the panel and copy local history, but has no OS target capabilities.
export class WebPasteboxService implements PasteboxService {
  private readonly key = 'voice-input-pastebox-preview';
  private readonly event = 'pastebox-preview-changed';
  async getState(query: PasteboxQuery = {}): Promise<PasteboxState> {
    let saved: { items?: PasteItem[]; mode?: PasteMode; notifications_enabled?: boolean } = {};
    try { saved = JSON.parse(localStorage.getItem(this.key) || '{}'); } catch { /* empty history */ }
    const all = saved.items || [];
    const pending = (item: PasteItem) => ['pending', 'failed', 'uncertain', 'delivering'].includes(item.delivery_status);
    const search = query.query?.trim().replace(/^#/, '') || '';
    const items = all.filter(item => (!query.beforeSeq || item.seq < query.beforeSeq) &&
      (!query.pendingOnly || pending(item)) && (!search || item.text.includes(search) || item.raw_text.includes(search) || String(item.seq) === search))
      .sort((a, b) => b.seq - a.seq).slice(0, 50);
    return { items, pending_count: all.filter(pending).length, mode: saved.mode || 'manual', targets: [],
      notifications_enabled: saved.notifications_enabled ?? true,
      opening_target_id: null, requested_item_id: null, requested_item: null,
      target_error: '浏览器预览支持复制；跨应用粘贴请使用 macOS 客户端。', notification_status: 'unavailable',
      accessibility_trusted: false, backend_error: null };
  }
  private update(mutator: (data: { items: PasteItem[]; mode: PasteMode; notifications_enabled?: boolean }) => void): void {
    const saved = JSON.parse(localStorage.getItem(this.key) || '{"items":[],"mode":"manual"}');
    mutator(saved);
    localStorage.setItem(this.key, JSON.stringify(saved));
    window.dispatchEvent(new Event(this.event));
  }
  async open(): Promise<void> { window.open('?pastebox=1', '_blank', 'width=440,height=620'); }
  async close(): Promise<void> { window.close(); }
  async setMode(mode: PasteMode): Promise<void> { this.update(data => { data.mode = mode; }); }
  async setNotificationsEnabled(enabled: boolean): Promise<void> { this.update(data => { data.notifications_enabled = enabled; }); }
  async act(action: PasteAction): Promise<PasteItem> {
    if (!action.copyOnly) throw new Error('跨应用粘贴需要 macOS 客户端');
    const saved = JSON.parse(localStorage.getItem(this.key) || '{"items":[]}');
    const item: PasteItem | undefined = saved.items.find((i: PasteItem) => i.id === action.id && i.version === action.version);
    if (!item) throw new Error('记录已更新，请刷新后重试');
    await navigator.clipboard.writeText(action.raw ? item.raw_text : item.text);
    const updated: PasteItem = { ...item, actioned: true, delivery_status: 'copied', version: item.version + 1 };
    this.update(data => { data.items = data.items.map(i => i.id === updated.id ? updated : i); });
    return updated;
  }
  async pasteNext(): Promise<null> { throw new Error('跨应用粘贴需要 macOS 客户端'); }
  async retry(): Promise<void> { throw new Error('重新转录需要 macOS 客户端'); }
  async delete(id: string, version: number): Promise<void> {
    this.update(data => { data.items = data.items.filter(item => !(item.id === id && item.version === version && item.processing_status === 'ready')); });
  }
  async clearHistory(): Promise<number> {
    const saved = JSON.parse(localStorage.getItem(this.key) || '{"items":[]}');
    const count = (saved.items || []).filter((item: PasteItem) => ['ready', 'failed'].includes(item.processing_status)).length;
    this.update(data => { data.items = (data.items || []).filter((item: PasteItem) => !['ready', 'failed'].includes(item.processing_status)); });
    return count;
  }
  async requestNotifications(): Promise<void> { throw new Error('系统通知需要 macOS 客户端'); }
  async requestAccessibility(): Promise<void> { throw new Error('辅助功能需要 macOS 客户端'); }
  async openMain(): Promise<void> { window.location.search = ''; }
  async quit(): Promise<void> { window.close(); }
  async onChange(listener: () => void): Promise<() => void> {
    window.addEventListener(this.event, listener); return () => window.removeEventListener(this.event, listener);
  }
  async onOpen(): Promise<() => void> { return () => undefined; }
}

const isDesktop = '__TAURI_INTERNALS__' in window || '__TAURI__' in window;
export const pasteboxService: PasteboxService = isDesktop ? new TauriPasteboxClient() : new WebPasteboxService();
