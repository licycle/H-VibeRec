import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { PasteAction, PasteItem, PasteMode, PasteboxQuery, PasteboxState } from '../types/pastebox';
import type { PasteboxService } from '../services/pastebox.service';

export class TauriPasteboxClient implements PasteboxService {
  getState(query: PasteboxQuery = {}): Promise<PasteboxState> { return invoke('get_pastebox_state', { ...query }); }
  open(): Promise<void> { return invoke('open_pastebox'); }
  close(): Promise<void> { return invoke('close_pastebox'); }
  setMode(mode: PasteMode): Promise<void> { return invoke('set_pastebox_mode', { mode }); }
  setNotificationsEnabled(enabled: boolean): Promise<void> { return invoke('set_pastebox_notifications_enabled', { enabled }); }
  act(action: PasteAction): Promise<PasteItem> { return invoke('act_on_pastebox_item', { ...action }); }
  pasteNext(): Promise<PasteItem | null> { return invoke('paste_next_item'); }
  async retry(id: string, version: number): Promise<void> { await invoke('retry_dictation_job', { id, version }); }
  delete(id: string, version: number): Promise<void> { return invoke('delete_pastebox_item', { id, version }); }
  clearHistory(): Promise<number> { return invoke('clear_pastebox_history'); }
  requestNotifications(): Promise<void> { return invoke('request_pastebox_notifications'); }
  requestAccessibility(): Promise<void> { return invoke('request_pastebox_accessibility'); }
  openMain(): Promise<void> { return invoke('open_main_window_from_voice_input_overlay'); }
  quit(): Promise<void> { return invoke('quit_from_pastebox'); }
  async onChange(listener: () => void): Promise<() => void> { return listen('pastebox-changed', listener); }
  async onOpen(listener: () => void): Promise<() => void> { return listen('pastebox-opened', listener); }
}
