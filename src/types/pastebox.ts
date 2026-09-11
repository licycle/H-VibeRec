export type PasteMode = 'manual' | 'automatic' | 'notify';
export interface PasteTarget {
  id: string;
  app_name: string;
  bundle_id: string;
  window_title: string;
  process_id?: number | null;
  window_id?: number | null;
  identity_method?: 'window_server' | 'ax_reference' | null;
  availability?: 'available' | 'unreachable' | 'closed' | null;
  unavailable_reason?: string | null;
  control_role?: string | null;
  capture_method?: string | null;
  capability?: 'exact_ax' | 'foreground_paste' | 'unavailable' | string | null;
  captured_at: string;
  available: boolean;
  selection_location: number;
  selection_length: number;
  caret_x: number | null;
  caret_y: number | null;
  window_bounds: [number, number, number, number] | null;
  caret_offset: [number, number] | null;
}
export interface PasteItem {
  id: string;
  seq: number;
  raw_text: string;
  text: string;
  processing_status: 'queued' | 'preparing_model' | 'transcribing' | 'refining' | 'ready' | 'failed';
  delivery_status: 'pending' | 'delivering' | 'verified' | 'sent' | 'copied' | 'failed' | 'uncertain';
  mode: PasteMode;
  target: PasteTarget | null;
  created_at: string;
  version: number;
  actioned: boolean;
  error: string | null;
  polish_error: string | null;
  notification_error: string | null;
}
export interface PasteboxState {
  notifications_enabled: boolean;
  items: PasteItem[];
  pending_count: number;
  mode: PasteMode;
  targets: PasteTarget[];
  opening_target_id: string | null;
  requested_item_id: string | null;
  requested_item: PasteItem | null;
  target_error: string | null;
  notification_status: 'granted' | 'denied' | 'not_determined' | 'unavailable';
  accessibility_trusted: boolean;
  backend_error: string | null;
}
export interface PasteboxQuery {
  beforeSeq?: number;
  query?: string;
  pendingOnly?: boolean;
}
export interface PasteAction {
  id: string;
  version: number;
  targetId?: string;
  raw: boolean;
  copyOnly: boolean;
}
