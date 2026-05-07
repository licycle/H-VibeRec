import { useState, useEffect } from 'react';
import { NoteDoc } from '../types';
import { Edit2, Check, X, Download, Import, RefreshCw, MoreVertical, Trash2, Brain, Loader2 } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { LocalQueueJob, SummaryTemplate } from '../appTypes';
import SummaryTemplatePicker from './SummaryTemplatePicker';
import './NotesList.css';

interface ImportedNote {
  title: string;
  content: string;
  file_name: string;
}

interface Props {
  notes: NoteDoc[];
  activeNoteId: string | null;
  onSelect: (id: string) => void;
  onRename?: (id: string, title: string) => void;
  onExport?: (note: NoteDoc) => void;
  onDelete?: (note: NoteDoc) => void;
  onNotesImported?: (notes: NoteDoc[]) => void | Promise<void>;
  onError?: (msg: string) => void;
  onRefresh?: () => void;
  workspaceFolder?: string;
  workspaceTitle?: string;
  onBeforeWorkspaceSummary?: () => Promise<NoteDoc[] | void> | NoteDoc[] | void;
}

type TextSummaryTarget =
  | { mode: 'workspace' }
  | { mode: 'note'; note: NoteDoc }
  | null;

export default function NotesList({
  notes,
  activeNoteId,
  onSelect,
  onRename,
  onExport,
  onDelete,
  onNotesImported,
  onError,
  onRefresh,
  workspaceFolder,
  workspaceTitle = '本地空间',
  onBeforeWorkspaceSummary,
}: Props) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingTitle, setEditingTitle] = useState('');
  const [isImporting, setIsImporting] = useState<boolean>(false);
  const [isSummarizingWorkspace, setIsSummarizingWorkspace] = useState<boolean>(false);
  const [activeMenuId, setActiveMenuId] = useState<string | null>(null);
  const [summaryTemplates, setSummaryTemplates] = useState<SummaryTemplate[]>([]);
  const [templatesLoading, setTemplatesLoading] = useState<boolean>(false);
  const [summaryTarget, setSummaryTarget] = useState<TextSummaryTarget>(null);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (activeMenuId && !(event.target as Element).closest('.actions-menu-container')) {
        setActiveMenuId(null);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [activeMenuId]);

  const startEdit = (n: NoteDoc) => {
    setEditingId(n.id);
    setEditingTitle(n.title || '');
  };
  const cancelEdit = () => { setEditingId(null); setEditingTitle(''); };
  const saveEdit = (id: string) => {
    const title = editingTitle.trim();
    if (title && onRename) onRename(id, title);
    cancelEdit();
  };

  const hasSummarizableText = notes.some(note => note.content.trim());

  const loadSummaryTemplates = async () => {
    setTemplatesLoading(true);
    try {
      const templates = await invoke<SummaryTemplate[]>('list_summary_templates');
      setSummaryTemplates(templates);
    } catch (error) {
      onError?.(typeof error === 'string' ? error : '加载总结模板失败');
    } finally {
      setTemplatesLoading(false);
    }
  };

  const openTextSummaryPicker = async (target: Exclude<TextSummaryTarget, null>) => {
    setActiveMenuId(null);
    setSummaryTarget(target);
    if (!summaryTemplates.length) {
      await loadSummaryTemplates();
    }
  };

  const summarizeTextTarget = async (template: SummaryTemplate) => {
    const target = summaryTarget;
    setSummaryTarget(null);
    if (!target) return;
    setIsSummarizingWorkspace(true);
    try {
      const refreshed = await onBeforeWorkspaceSummary?.();
      const sourceNotes = Array.isArray(refreshed) ? refreshed : notes;
      const targetNotes = target.mode === 'note'
        ? [sourceNotes.find(note => note.id === target.note.id) || target.note]
        : sourceNotes;
      const documents = targetNotes
        .filter(note => note.content.trim())
        .map(note => ({
          title: note.title || '未命名笔记',
          content: note.content,
        }));

      if (!documents.length) {
        throw new Error(target.mode === 'note' ? '当前笔记没有可总结的文本' : '当前本地空间没有可总结的文本文件');
      }

      const title = target.mode === 'note'
        ? `总结 - ${documents[0].title} - ${template.name}`
        : `${workspaceTitle} 本地空间总结 - ${template.name}`;
      await invoke<LocalQueueJob>('enqueue_workspace_text_summary', {
        templateId: template.id,
        workspaceFolder,
        workspaceTitle: target.mode === 'note' ? documents[0].title : workspaceTitle,
        documents,
        title,
        summaryScope: target.mode === 'note' ? 'single_text' : 'workspace_text',
      });
      onRefresh?.();
    } catch (error) {
      onError?.(error instanceof Error ? error.message : typeof error === 'string' ? error : '本地空间总结失败');
    } finally {
      setIsSummarizingWorkspace(false);
    }
  };

  const importNoteFiles = async () => {
    try {
      setIsImporting(true);

      // Open file dialog for multiple note files
      const files = await open({
        title: '导入笔记文件',
        multiple: true,
        filters: [{
          name: '文档文件',
          extensions: ['md', 'txt', 'docx']
        }]
      });

      if (!files || (Array.isArray(files) && files.length === 0)) {
        setIsImporting(false);
        return;
      }

      // Convert to array if single file
      const filePaths = Array.isArray(files) ? files : [files];

      // Call backend to import files
      const importedNotes = await invoke<ImportedNote[]>('import_note_files', { filePaths });

      // Convert imported notes to NoteDoc format
      const newNotes: NoteDoc[] = importedNotes.map(note => ({
        id: crypto.randomUUID(),
        title: note.title,
        content: note.content,
        created: new Date().toISOString(),
        updated: new Date().toISOString(),
      }));

      // Call parent callback to add notes
      if (onNotesImported) {
        await onNotesImported(newNotes);
      }
    } catch (error) {
      if (onError) {
        onError(typeof error === 'string' ? error : '导入笔记文件失败');
      }
    } finally {
      setIsImporting(false);
    }
  };

  return (
    <div className="notes-list">
      <div className="notes-header">
        <button
          className="refresh-button"
          onClick={() => onRefresh?.()}
          title="刷新列表"
        >
          <RefreshCw size={16} />
        </button>
        <button
          className="summary-button"
          onClick={() => void openTextSummaryPicker({ mode: 'workspace' })}
          disabled={!hasSummarizableText || isSummarizingWorkspace}
          title="总结当前本地空间中的全部文本文件"
        >
          {isSummarizingWorkspace ? <Loader2 size={16} className="spinning" /> : <Brain size={16} />}
          本地空间总结
        </button>
        <button
          className="import-button"
          onClick={importNoteFiles}
          disabled={isImporting}
          title="导入笔记文件"
        >
          <Import size={16} />
          {isImporting ? '导入中...' : '导入笔记'}
        </button>
      </div>

      <div className="notes-content">
        {notes.length === 0 ? (
          <div className="empty-state">
            <h2>暂无笔记</h2>
            <p>点击中间区域"新建笔记"创建，或导入现有文档</p>
          </div>
        ) : (
          notes.map(n => (
          <div key={n.id} className={`note-item ${n.id === activeNoteId ? 'active' : ''}`} onClick={() => onSelect(n.id)}>
            {editingId === n.id ? (
              <div className="meeting-edit" onClick={e => e.stopPropagation()}>
                <input
                  className="meeting-input"
                  value={editingTitle}
                  onChange={e => setEditingTitle(e.target.value)}
                  onKeyDown={e => {
                    if (e.key === 'Enter') saveEdit(n.id);
                    if (e.key === 'Escape') cancelEdit();
                  }}
                  autoFocus
                />
                <div className="edit-actions">
                  <button className="mini-btn" onClick={(e) => { e.stopPropagation(); saveEdit(n.id); }} title="保存"><Check size={12} /></button>
                  <button className="mini-btn" onClick={(e) => { e.stopPropagation(); cancelEdit(); }} title="取消"><X size={12} /></button>
                </div>
              </div>
            ) : (
              <>
                <div className="note-main">
                  <div className="note-title-container">
                    <div className="note-title" title={n.title}>{n.title || '未命名笔记'}</div>
                  </div>
                  <div className="note-meta">{new Date(n.updated || n.created).toLocaleString()}</div>
                </div>
                <div className="note-actions">
                  <div className="actions-menu-container">
                    <button
                      className="mini-btn"
                      onClick={(e) => {
                        e.stopPropagation();
                        setActiveMenuId(activeMenuId === n.id ? null : n.id);
                      }}
                      title="更多操作"
                    >
                      <MoreVertical size={14} />
                    </button>
                    {activeMenuId === n.id && (
                      <div className="actions-menu" onClick={(e) => e.stopPropagation()}>
                        <button
                          className="menu-item"
                          onClick={() => {
                            startEdit(n);
                            setActiveMenuId(null);
                          }}
                        >
                          <Edit2 size={14} /> 重命名
                        </button>
                        <button
                          className="menu-item"
                          onClick={() => {
                            void openTextSummaryPicker({ mode: 'note', note: n });
                          }}
                          disabled={!n.content.trim() || isSummarizingWorkspace}
                        >
                          <Brain size={14} /> 总结
                        </button>
                        <button
                          className="menu-item"
                          onClick={() => {
                            onExport?.(n);
                            setActiveMenuId(null);
                          }}
                        >
                          <Download size={14} /> 导出
                        </button>
                        <button
                          className="menu-item danger"
                          onClick={() => {
                            onDelete?.(n);
                            setActiveMenuId(null);
                          }}
                        >
                          <Trash2 size={14} /> 删除
                        </button>
                      </div>
                    )}
                  </div>
                </div>
              </>
            )}
          </div>
          ))
        )}
      </div>
      <SummaryTemplatePicker
        open={summaryTarget !== null}
        title={summaryTarget?.mode === 'note' ? '选择单个文本总结模板' : '选择本地空间总结模板'}
        description={
          summaryTarget?.mode === 'note'
            ? `将"${summaryTarget.note.title || '未命名笔记'}"作为单个文本文件总结。`
            : `将"${workspaceTitle}"中的全部文本笔记作为一个整体总结。`
        }
        templates={summaryTemplates}
        loading={templatesLoading}
        confirmLabel="开始总结"
        onRefresh={loadSummaryTemplates}
        onCancel={() => setSummaryTarget(null)}
        onConfirm={summarizeTextTarget}
      />
    </div>
  );
}
