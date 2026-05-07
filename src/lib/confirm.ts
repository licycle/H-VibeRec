type ConfirmOptions = {
  title?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  destructive?: boolean;
};

export function confirmLocalAction(message: string, options: ConfirmOptions = {}): Promise<boolean> {
  if (typeof document === 'undefined') {
    return Promise.resolve(false);
  }

  return new Promise(resolve => {
    const title = options.title || '确认操作';
    const confirmLabel = options.confirmLabel || '确认';
    const cancelLabel = options.cancelLabel || '取消';

    const overlay = document.createElement('div');
    overlay.setAttribute('role', 'presentation');
    Object.assign(overlay.style, {
      position: 'fixed',
      inset: '0',
      zIndex: '2147483647',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      padding: '20px',
      background: 'rgba(15, 23, 42, 0.42)',
    });

    const panel = document.createElement('div');
    panel.setAttribute('role', 'dialog');
    panel.setAttribute('aria-modal', 'true');
    panel.setAttribute('aria-labelledby', 'local-confirm-title');
    Object.assign(panel.style, {
      width: 'min(420px, 100%)',
      borderRadius: '8px',
      background: 'var(--bg-primary, #fff)',
      color: 'var(--text-primary, #111827)',
      boxShadow: '0 20px 60px rgba(15, 23, 42, 0.28)',
      border: '1px solid var(--border-color, rgba(148, 163, 184, 0.35))',
      padding: '20px',
    });

    const titleEl = document.createElement('div');
    titleEl.id = 'local-confirm-title';
    titleEl.textContent = title;
    Object.assign(titleEl.style, {
      fontSize: '1rem',
      fontWeight: '700',
      marginBottom: '10px',
    });

    const messageEl = document.createElement('div');
    messageEl.textContent = message;
    Object.assign(messageEl.style, {
      fontSize: '0.875rem',
      lineHeight: '1.6',
      color: 'var(--text-secondary, #374151)',
      marginBottom: '18px',
      whiteSpace: 'pre-wrap',
    });

    const actions = document.createElement('div');
    Object.assign(actions.style, {
      display: 'flex',
      justifyContent: 'flex-end',
      gap: '10px',
    });

    const cancelButton = document.createElement('button');
    cancelButton.type = 'button';
    cancelButton.textContent = cancelLabel;
    Object.assign(cancelButton.style, {
      minWidth: '76px',
      height: '34px',
      borderRadius: '6px',
      border: '1px solid var(--border-color, #d1d5db)',
      background: 'var(--bg-secondary, #f9fafb)',
      color: 'var(--text-primary, #111827)',
      cursor: 'pointer',
      fontSize: '0.8125rem',
    });

    const confirmButton = document.createElement('button');
    confirmButton.type = 'button';
    confirmButton.textContent = confirmLabel;
    Object.assign(confirmButton.style, {
      minWidth: '76px',
      height: '34px',
      borderRadius: '6px',
      border: '1px solid transparent',
      background: options.destructive ? '#dc2626' : 'var(--accent-color, #2563eb)',
      color: '#fff',
      cursor: 'pointer',
      fontSize: '0.8125rem',
      fontWeight: '600',
    });

    const cleanup = (accepted: boolean) => {
      document.removeEventListener('keydown', handleKeyDown);
      overlay.remove();
      resolve(accepted);
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        cleanup(false);
      }
    };

    overlay.addEventListener('mousedown', event => {
      if (event.target === overlay) {
        cleanup(false);
      }
    });
    cancelButton.addEventListener('click', () => cleanup(false));
    confirmButton.addEventListener('click', () => cleanup(true));
    document.addEventListener('keydown', handleKeyDown);

    actions.append(cancelButton, confirmButton);
    panel.append(titleEl, messageEl, actions);
    overlay.append(panel);
    document.body.append(overlay);
    cancelButton.focus();
  });
}
