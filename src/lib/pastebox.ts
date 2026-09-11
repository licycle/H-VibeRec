import type { PasteItem, PasteMode, PasteboxState } from '../types/pastebox';

export const PASTE_MODES: { value: PasteMode; label: string; description: string }[] = [
  { value: 'manual', label: '手动', description: '内容保存到粘贴箱，点击条目粘贴或仅复制。' },
  { value: 'automatic', label: '自动', description: '按录音顺序回写各自的原位置；录音期间暂停回写，结束后自动继续。点击完成通知仅返回位置。' },
  { value: 'notify', label: '半自动', description: '完成后保存在粘贴箱；点击对应通知恢复本次开始位置并粘贴。' },
];

export function pasteStatus(item: PasteItem): string {
  if (item.processing_status === 'queued') return '等待转录';
  if (item.processing_status === 'preparing_model') return '准备语音模型';
  if (item.processing_status === 'transcribing') return '转录中';
  if (item.processing_status === 'failed') return '转录未完成 · 可重试';
  if (item.delivery_status === 'pending' && item.processing_status === 'refining') return '润色中 · 原文可用';
  if (item.delivery_status === 'pending' && item.mode === 'automatic') return '等待自动回写';
  return {
    pending: '待粘贴', delivering: '正在粘贴', verified: '已粘贴', sent: '已发送粘贴',
    copied: '已复制', failed: '粘贴未完成', uncertain: '请检查上次粘贴',
  }[item.delivery_status];
}

export function resolvePasteTarget(item: PasteItem, state: PasteboxState, choice: string): string {
  if (choice === 'original') {
    if (!item.target?.available) throw new Error('本条原位置不可用，请在目标输入框放好光标后重新打开顶部栏粘贴箱，选择打开前的位置，或仅复制。');
    return item.target.id;
  }
  const id = choice === 'opening' ? state.opening_target_id : choice;
  if (!id || !state.targets.some(target => target.id === id && target.available)) {
    throw new Error('没有可用的位置，请在其他应用放好文字光标，再打开菜单栏粘贴箱。');
  }
  return id;
}

export function mergePasteItems(previous: PasteItem[], next: PasteItem[], append: boolean): PasteItem[] {
  const items = new Map((append ? previous : []).map(item => [item.id, item]));
  next.forEach(item => items.set(item.id, item));
  return [...items.values()].sort((a, b) => b.seq - a.seq);
}
