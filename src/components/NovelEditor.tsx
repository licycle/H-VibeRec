import { useEffect, useRef, useCallback, useState } from 'react';
import { useEditor, EditorContent, type Editor } from '@tiptap/react';
import StarterKit from '@tiptap/starter-kit';
import Placeholder from '@tiptap/extension-placeholder';
import TaskList from '@tiptap/extension-task-list';
import TaskItem from '@tiptap/extension-task-item';
import Underline from '@tiptap/extension-underline';
import Link from '@tiptap/extension-link';
import Highlight from '@tiptap/extension-highlight';
import { defaultMarkdownParser, defaultMarkdownSerializer } from 'prosemirror-markdown';
import './NovelEditor.css';

interface NovelEditorProps {
  value: string;
  onChange: (value: string) => void;
  height?: number | string;
  noteId?: string;
}

export default function NovelEditor({
  value,
  onChange,
  height = '100%',
  noteId
}: NovelEditorProps) {
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastContentRef = useRef<string>('');
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const isInitialized = useRef(false);

  // 强制将页面滚动位置复位到顶部
  const resetPageScroll = () => {
    try {
      window.scrollTo({ top: 0, left: 0, behavior: 'auto' });
      const de = document.documentElement as HTMLElement;
      (de as any).scrollTop = 0;
      (document.body as any).scrollTop = 0;
    } catch {}
  };

  // 将 Markdown 转换为 ProseMirror JSON
  const markdownToJSON = useCallback((markdown: string) => {
    try {
      if (!markdown) {
        return null;
      }
      const doc = defaultMarkdownParser.parse(markdown);
      return doc?.toJSON() || null;
    } catch (error) {
      console.error('Error converting markdown to JSON:', error);
      return null;
    }
  }, []);

  // 将 ProseMirror 节点转换为 Markdown
  const nodeToMarkdown = useCallback((doc: any): string => {
    try {
      if (!doc) return '';
      return defaultMarkdownSerializer.serialize(doc);
    } catch (error) {
      console.error('Error converting to markdown:', error);
      return '';
    }
  }, []);

  // 防抖的onChange处理
  const debouncedOnChange = useCallback((content: string) => {
    if (debounceTimerRef.current) {
      clearTimeout(debounceTimerRef.current);
    }
    lastContentRef.current = content;
    debounceTimerRef.current = setTimeout(() => {
      onChange(content);
    }, 500);
  }, [onChange]);

  // 初始化编辑器
  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        history: {
          depth: 100, // 限制历史记录深度
        },
      }),
      Placeholder.configure({
        placeholder: '在此输入会议笔记...',
      }),
      TaskList,
      TaskItem.configure({
        nested: true,
      }),
      Underline,
      Link.configure({
        openOnClick: false,
      }),
      Highlight,
    ],
    content: markdownToJSON(value),
    editorProps: {
      attributes: {
        class: 'novel-editor-prose',
      },
    },
    onUpdate: ({ editor }: { editor: Editor }) => {
      const markdown = nodeToMarkdown(editor.state.doc);
      if (markdown !== lastContentRef.current) {
        debouncedOnChange(markdown);
      }
    },
  }, [noteId]); // 使用 noteId 作为依赖，这样切换笔记时会重新创建编辑器

  // 处理外部值变化
  useEffect(() => {
    if (!editor) return;

    // 首次初始化后跳过
    if (!isInitialized.current) {
      isInitialized.current = true;
      return;
    }

    const newValue = value || '';

    // 如果内容没有变化，不需要更新
    if (newValue === lastContentRef.current) return;

    // 显示大文档的加载指示器
    const isLargeDocument = newValue.length > 5000;
    const isVeryLargeDocument = newValue.length > 30000;

    if (isVeryLargeDocument) {
      console.warn(`Loading very large document (${Math.floor(newValue.length / 1000)}k chars).`);
    }

    if (isLargeDocument) {
      setIsLoading(true);
    }

    // 转换并设置内容
    setTimeout(() => {
      try {
        const jsonContent = markdownToJSON(newValue);
        if (jsonContent && editor) {
          editor.commands.setContent(jsonContent);
          lastContentRef.current = newValue;
        }
      } catch (error) {
        console.error('Error setting editor content:', error);
      } finally {
        setIsLoading(false);
      }
    }, isLargeDocument ? 100 : 0);

    return () => {
      setIsLoading(false);
    };
  }, [value, editor, markdownToJSON]);

  // 清理
  useEffect(() => {
    resetPageScroll();

    return () => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
      resetPageScroll();
    };
  }, []);

  return (
    <div
      className="novel-editor-container"
      style={{
        position: 'relative',
        height: typeof height === 'number' ? `${height}px` : height
      }}
    >
      {isLoading && (
        <div className="novel-editor-loading">
          <div className="novel-editor-loading-content">
            加载中...
          </div>
        </div>
      )}
      <div style={{ height: '100%', overflow: 'auto' }}>
        <EditorContent editor={editor} className="novel-editor-content" />
      </div>
    </div>
  );
}
