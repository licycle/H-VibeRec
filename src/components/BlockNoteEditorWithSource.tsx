import { useState, useRef, useCallback, useEffect, type CSSProperties, type ClipboardEvent } from 'react';
import { PartialBlock } from '@blocknote/core';
import { BlockNoteView } from '@blocknote/mantine';
import { useCreateBlockNote } from '@blocknote/react';
import CodeMirror from '@uiw/react-codemirror';
import { markdown } from '@codemirror/lang-markdown';
import { oneDark } from '@codemirror/theme-one-dark';
import { invoke } from '@tauri-apps/api/core';
import '@blocknote/mantine/style.css';
import './BlockNoteEditorWithSource.css';

export type EditorMode = 'wysiwyg' | 'source';

const DEBUG_PREVIEW_CHARS = 120;

interface BlockNoteEditorWithSourceProps {
  value: string;
  onChange: (value: string) => void;
  height?: number | string;
  noteId?: string;
  mode: EditorMode;
  darkMode: boolean;
  readOnly?: boolean;
  compact?: boolean;
  fontScale?: number;
}

export default function BlockNoteEditorWithSource({
  value,
  onChange,
  height = '100%',
  noteId,
  mode,
  darkMode,
  readOnly = false,
  compact = false,
  fontScale = 1,
}: BlockNoteEditorWithSourceProps) {
  const [sourceContent, setSourceContent] = useState(value);
  const previousModeRef = useRef<EditorMode>(mode);
  const renderMode: EditorMode = readOnly ? 'wysiwyg' : mode;

  // Track if content has changed but not saved yet
  const hasUnsavedChangesRef = useRef(false);
  const currentContentRef = useRef(value);
  const isLoadingDocumentRef = useRef(false);
  const lastPasteAtRef = useRef(0);

  // Create BlockNote editor instance
  const editor = useCreateBlockNote({
    initialContent: undefined,
  });

  // Convert editor content to markdown
  const getMarkdownFromEditor = useCallback(async (): Promise<string> => {
    if (!editor) return '';
    try {
      const blocks = editor.document;
      return await editor.blocksToMarkdownLossy(blocks);
    } catch (error) {
      console.error('[BlockNote] Failed to convert to markdown:', error);
      return '';
    }
  }, [editor]);

  const replaceEditorContent = useCallback((blocks: PartialBlock[]) => {
    if (!editor) return;

    const currentBlocks = editor.document;
    editor.replaceBlocks(currentBlocks, blocks);
  }, [editor]);

  // Save current content (only when not focused)
  const saveContent = useCallback(async () => {
    if (!hasUnsavedChangesRef.current) return;

    const content = currentContentRef.current;
    onChange(content);
    hasUnsavedChangesRef.current = false;
    logVoiceInputEditorEvent(
      'AutoSave content saved',
      { noteId, mode: renderMode },
      textDebugSummary('content', content)
    );
  }, [onChange, noteId, renderMode]);

  // Handle editor content changes (WYSIWYG mode) - only track changes, don't save immediately
  useEffect(() => {
    if (!editor || renderMode !== 'wysiwyg' || readOnly) return;

    const handleChange = async () => {
      if (isLoadingDocumentRef.current) return;

      // Get current markdown
      const markdown = await getMarkdownFromEditor();

      // Update local state immediately for responsive editing
      setSourceContent(markdown);
      currentContentRef.current = markdown;
      hasUnsavedChangesRef.current = true;
      if (Date.now() - lastPasteAtRef.current < 3_000) {
        logVoiceInputEditorEvent(
          'WYSIWYG markdown after paste',
          { noteId, mode: renderMode },
          textDebugSummary('markdown', markdown)
        );
      }
    };

    // Listen to editor changes
    return editor.onChange(handleChange);
  }, [editor, renderMode, readOnly, getMarkdownFromEditor, noteId]);

  // Handle mode switching - save before switching
  useEffect(() => {
    if (!editor) return;

    const handleModeSwitch = async () => {
      if (renderMode !== previousModeRef.current) {
        // Save before mode switch
        await saveContent();

        if (renderMode === 'source') {
          // Switching to source mode: get current markdown from WYSIWYG
          const markdown = await getMarkdownFromEditor();
          setSourceContent(markdown);
          currentContentRef.current = markdown;
          console.log('[EditorMode] Switched to source mode');
        } else if (renderMode === 'wysiwyg') {
          // Switching to WYSIWYG mode: parse source content back to blocks
          try {
            isLoadingDocumentRef.current = true;
            const blocks = await editor.tryParseMarkdownToBlocks(sourceContent);
            replaceEditorContent(blocks);
            currentContentRef.current = sourceContent;
            hasUnsavedChangesRef.current = false;
            console.log('[EditorMode] Switched to WYSIWYG mode');
          } catch (error) {
            console.error('[EditorMode] Failed to parse markdown:', error);
          } finally {
            isLoadingDocumentRef.current = false;
          }
        }
        previousModeRef.current = renderMode;
      }
    };

    handleModeSwitch();
  }, [renderMode, editor, sourceContent, getMarkdownFromEditor, saveContent, replaceEditorContent]);

  // Initialize editor with content when noteId or value changes.
  useEffect(() => {
    if (!editor) return;

    let cancelled = false;
    const nextValue = value || '';

    const initializeContent = async () => {
      try {
        isLoadingDocumentRef.current = true;
        const blocks = nextValue.trim()
          ? await editor.tryParseMarkdownToBlocks(nextValue)
          : [{ type: 'paragraph', content: '' } as PartialBlock];
        if (cancelled) return;
        replaceEditorContent(blocks);
        setSourceContent(nextValue);
        currentContentRef.current = nextValue;
        hasUnsavedChangesRef.current = false;
      } catch (error) {
        console.error('[BlockNote] Failed to initialize content:', error);
      } finally {
        if (!cancelled) {
          isLoadingDocumentRef.current = false;
        }
      }
    };

    void initializeContent();

    return () => {
      cancelled = true;
    };
  }, [noteId, value, editor, replaceEditorContent]);

  // Source mode content change - only track changes, don't save immediately
  const handleSourceChange = useCallback((newValue: string) => {
    if (readOnly) return;
    // Update local state immediately for responsive editing
    setSourceContent(newValue);
    currentContentRef.current = newValue;
    hasUnsavedChangesRef.current = true;
    if (Date.now() - lastPasteAtRef.current < 3_000) {
      logVoiceInputEditorEvent(
        'Source markdown after paste',
        { noteId, mode: renderMode },
        textDebugSummary('markdown', newValue)
      );
    }
  }, [readOnly, noteId, renderMode]);

  const handlePasteCapture = useCallback((event: ClipboardEvent<HTMLDivElement>) => {
    lastPasteAtRef.current = Date.now();
    const clipboardText = event.clipboardData?.getData('text/plain') || '';
    const target = event.target instanceof HTMLElement
      ? {
          tag: event.target.tagName,
          className: event.target.className?.toString?.() || '',
          role: event.target.getAttribute('role') || '',
          contentEditable: event.target.getAttribute('contenteditable') || '',
        }
      : null;

    logVoiceInputEditorEvent(
      'Paste captured',
      { noteId, mode: renderMode, isTrusted: event.isTrusted, target },
      textDebugSummary('clipboard', clipboardText)
    );
  }, [noteId, renderMode]);

  // Handle blur event to save content when focus is lost
  const handleBlur = useCallback(() => {
    if (readOnly) return;
    console.log('[AutoSave] Editor blur detected, saving...');
    saveContent();
  }, [readOnly, saveContent]);

  const rootStyle = {
    '--blocknote-font-scale': String(fontScale),
  } as CSSProperties;

  return (
    <div
      className={`blocknote-editor-with-source ${readOnly ? 'read-only' : ''} ${compact ? 'compact' : ''}`}
      onBlur={handleBlur}
      onPasteCapture={handlePasteCapture}
      style={rootStyle}
    >
      {/* WYSIWYG Editor */}
      <div style={{ display: renderMode === 'wysiwyg' ? 'block' : 'none', height: '100%' }}>
        <BlockNoteView
          editor={editor}
          theme={darkMode ? 'dark' : 'light'}
          editable={!readOnly}
          formattingToolbar={!readOnly}
          linkToolbar={!readOnly}
          slashMenu={!readOnly}
          sideMenu={!readOnly}
          filePanel={!readOnly}
          tableHandles={!readOnly}
          emojiPicker={!readOnly}
          data-theming-css-variables-demo
        />
      </div>

      {/* Source Editor */}
      {renderMode === 'source' && (
        <CodeMirror
          value={sourceContent}
          height={typeof height === 'number' ? `${height}px` : height}
          extensions={[markdown()]}
          onChange={handleSourceChange}
          theme={darkMode ? oneDark : 'light'}
          basicSetup={{
            lineNumbers: true,
            highlightActiveLineGutter: true,
            highlightSpecialChars: true,
            foldGutter: true,
            drawSelection: true,
            dropCursor: true,
            allowMultipleSelections: true,
            indentOnInput: true,
            bracketMatching: true,
            closeBrackets: true,
            autocompletion: true,
            rectangularSelection: true,
            crosshairCursor: true,
            highlightActiveLine: true,
            highlightSelectionMatches: true,
            closeBracketsKeymap: true,
            searchKeymap: true,
            foldKeymap: true,
            completionKeymap: true,
            lintKeymap: true,
          }}
        />
      )}
    </div>
  );
}

function textDebugSummary(label: string, text: string): string {
  return `${label}: chars=${Array.from(text).length} bytes=${utf8ByteLength(text)} lines=${lineCount(text)} hash=${textHash(text)} preview="${previewText(text)}"`;
}

function textHash(text: string): string {
  let hash = 0x811c9dc5;
  const bytes = new TextEncoder().encode(text);
  for (const byte of bytes) {
    hash ^= byte;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, '0');
}

function utf8ByteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}

function lineCount(text: string): number {
  if (text.length === 0) return 0;
  const lines = text.replace(/\r\n/g, '\n').replace(/\r/g, '\n').split('\n');
  if (lines[lines.length - 1] === '') {
    lines.pop();
  }
  return lines.length;
}

function previewText(text: string): string {
  const chars = Array.from(text);
  const preview = chars.slice(0, DEBUG_PREVIEW_CHARS).join('') + (chars.length > DEBUG_PREVIEW_CHARS ? '...' : '');
  return preview
    .replace(/\\/g, '\\\\')
    .replace(/\n/g, '\\n')
    .replace(/\r/g, '\\r')
    .replace(/\t/g, '\\t');
}

function logVoiceInputEditorEvent(
  message: string,
  details: Record<string, unknown>,
  summary: string
): void {
  const event = `${message} details=${safeJson(details)} ${summary}`;
  console.info('[VoiceInputEditor]', event);
  void invoke('log_voice_input_frontend_event', { event }).catch(error => {
    console.warn('[VoiceInputEditor] Failed to bridge frontend log', error);
  });
}

function safeJson(value: unknown): string {
  try {
    return JSON.stringify(value);
  } catch {
    return '"<unserializable>"';
  }
}
