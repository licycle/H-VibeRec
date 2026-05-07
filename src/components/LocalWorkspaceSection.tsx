/**
 * Local Workspace Section Component
 * Displays the single local workspace (git-like working directory)
 */

import { useState } from 'react';
import { LocalWorkspace } from '../types';
import { Check, Edit2, FileEdit, MoreVertical, Plus, Trash2, X } from 'lucide-react';
import { confirmLocalAction } from '../lib/confirm';
import './LocalWorkspaceSection.css';

interface LocalWorkspaceSectionProps {
  workspaces: LocalWorkspace[];
  selectedWorkspaceId: string;
  onSelect: (workspaceId: string) => void;
  onCreate: (title: string) => void;
  onRename: (workspaceId: string, title: string) => void;
  onDelete: (workspaceId: string) => void | Promise<void>;
}

export function LocalWorkspaceSection({
  workspaces,
  selectedWorkspaceId,
  onSelect,
  onCreate,
  onRename,
  onDelete,
}: LocalWorkspaceSectionProps) {
  const [activeMenuId, setActiveMenuId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingTitle, setEditingTitle] = useState('');

  const startRename = (workspace: LocalWorkspace) => {
    setEditingId(workspace.id);
    setEditingTitle(workspace.title);
    setActiveMenuId(null);
  };

  const saveRename = () => {
    if (!editingId) return;
    const title = editingTitle.trim();
    if (title) onRename(editingId, title);
    setEditingId(null);
    setEditingTitle('');
  };

  const cancelRename = () => {
    setEditingId(null);
    setEditingTitle('');
  };

  const createWorkspace = () => {
    const count = workspaces.length + 1;
    onCreate(`本地工作区 ${count}`);
  };

  const deleteWorkspace = async (workspace: LocalWorkspace) => {
    setActiveMenuId(null);
    if (workspaces.length <= 1) return;
    const accepted = await confirmLocalAction(`确定要删除"${workspace.title}"吗？该空间下的笔记和录音文件会一并删除。`, {
      title: '删除本地空间',
      confirmLabel: '删除',
      destructive: true,
    });
    if (accepted) onDelete(workspace.id);
  };

  return (
    <div className="local-workspace-section">
      <div className="section-header">
        <div className="section-header-content">
          <h3 className="section-title">本地空间</h3>
          <span className="section-subtitle">按文件夹划分工作区</span>
        </div>
        <button className="icon-btn create-btn" onClick={createWorkspace} title="新建本地空间">
          <Plus size={16} />
        </button>
      </div>

      <div className="local-workspace-list">
        {workspaces.map(workspace => {
          const isSelected = workspace.id === selectedWorkspaceId;
          const isEditing = editingId === workspace.id;
          return (
            <div
              key={workspace.id}
              className={`workspace-item ${isSelected ? 'selected' : ''}`}
              onClick={() => !isEditing && onSelect(workspace.id)}
              role="button"
              tabIndex={0}
              onKeyDown={(e) => {
                if (!isEditing && (e.key === 'Enter' || e.key === ' ')) {
                  e.preventDefault();
                  onSelect(workspace.id);
                }
              }}
            >
              <div className="workspace-icon">
                <FileEdit size={18} />
              </div>
              <div className="workspace-info">
                {isEditing ? (
                  <div className="workspace-edit" onClick={event => event.stopPropagation()}>
                    <input
                      className="workspace-input"
                      value={editingTitle}
                      onChange={event => setEditingTitle(event.target.value)}
                      onKeyDown={event => {
                        if (event.key === 'Enter') saveRename();
                        if (event.key === 'Escape') cancelRename();
                      }}
                      autoFocus
                    />
                    <button className="mini-btn" onClick={saveRename} title="保存">
                      <Check size={12} />
                    </button>
                    <button className="mini-btn" onClick={cancelRename} title="取消">
                      <X size={12} />
                    </button>
                  </div>
                ) : (
                  <>
                    <div className="workspace-title">{workspace.title}</div>
                    <div className="workspace-status">{workspace.folderName}</div>
                  </>
                )}
              </div>
              {!isEditing && (
                <div className="actions-menu-container" onClick={event => event.stopPropagation()}>
                  <button
                    className="mini-btn"
                    onClick={() => setActiveMenuId(activeMenuId === workspace.id ? null : workspace.id)}
                    title="空间操作"
                  >
                    <MoreVertical size={14} />
                  </button>
                  {activeMenuId === workspace.id && (
                    <div className="actions-menu">
                      <button className="menu-item" onClick={() => startRename(workspace)}>
                        <Edit2 size={14} /> 重命名
                      </button>
                      <button
                        className="menu-item danger"
                        onClick={() => void deleteWorkspace(workspace)}
                        disabled={workspaces.length <= 1}
                      >
                        <Trash2 size={14} /> 删除
                      </button>
                    </div>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
