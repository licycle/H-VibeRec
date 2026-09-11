// Vite-only integration fixture: uses the production editors in the real Tauri
// main WebView, with disposable content and no notes/settings database access.
import React from 'react';
import { createRoot } from 'react-dom/client';
import { emit } from '@tauri-apps/api/event';
import BlockNoteEditorWithSource from '../../src/components/BlockNoteEditorWithSource';

const initial = '甲😀乙';

function FixtureEditor({ mode, noteId }: { mode: 'wysiwyg' | 'source'; noteId: string }) {
  const [value, setValue] = React.useState(initial);
  return <BlockNoteEditorWithSource value={value} onChange={setValue} noteId={noteId}
    mode={mode} darkMode={false} />;
}

createRoot(document.getElementById('root')!).render(
  <main style={{ padding: 24 }}>
    <textarea id="plain" defaultValue={initial} aria-label="自身普通输入框" />
    <section id="rich" style={{ height: 200 }}>
      <FixtureEditor noteId="audit-rich" mode="wysiwyg" />
    </section>
    <section id="source" style={{ height: 200 }}>
      <FixtureEditor noteId="audit-source" mode="source" />
    </section>
  </main>
);

type Request = { event: string; op: 'select' | 'read'; kind: 'plain' | 'rich' | 'source'; location?: number };
Object.assign(window, {
  async hvrTargetFixture(request: Request) {
    try {
      const selector = request.kind === 'plain' ? '#plain' :
        request.kind === 'rich' ? '#rich [contenteditable="true"]' : '#source .cm-content';
      const element = document.querySelector<HTMLElement>(selector);
      if (!element || !element.textContent && !(element instanceof HTMLTextAreaElement)) {
        throw new Error('Editor is not ready');
      }
      if (request.op === 'select') {
        element.focus();
        const location = request.location ?? 3;
        if (element instanceof HTMLTextAreaElement) {
          element.setSelectionRange(location, location);
        } else {
          const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
          let node = walker.nextNode();
          let remaining = location;
          while (node && remaining > (node.textContent?.length || 0)) {
            remaining -= node.textContent?.length || 0;
            node = walker.nextNode();
          }
          if (!node) throw new Error('Missing editable text node');
          const range = document.createRange();
          range.setStart(node, remaining);
          range.collapse(true);
          const selection = window.getSelection()!;
          selection.removeAllRanges();
          selection.addRange(range);
          document.dispatchEvent(new Event('selectionchange'));
        }
      }
      const text = element instanceof HTMLTextAreaElement ? element.value : element.textContent;
      await emit(request.event, { text, active: element === document.activeElement });
    } catch (error) {
      await emit(request.event, { error: String(error) });
    }
  },
});
