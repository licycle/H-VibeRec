/**
 * Meeting Sidebar Component (Refactored)
 * Displays the local workspace for the single-machine app.
 */

import { Settings, ChevronLeft, ChevronRight } from 'lucide-react';
import { LocalWorkspace } from '../types';
import { LocalWorkspaceSection } from './LocalWorkspaceSection';
import VoiceInputStatsPanel from './VoiceInputStatsPanel';
import './MeetingSidebar.css';

interface MeetingSidebarProps {
  localWorkspaces: LocalWorkspace[];
  localWorkspace: LocalWorkspace;
  onSelectLocalWorkspace: (workspaceId: string) => void;
  onCreateLocalWorkspace: (title: string) => void;
  onRenameLocalWorkspace: (workspaceId: string, title: string) => void;
  onDeleteLocalWorkspace: (workspaceId: string) => void | Promise<void>;
  leftOpen: boolean;
  setLeftOpen: (open: boolean) => void;
  onOpenSettings: () => void;
}

export default function MeetingSidebar({
  localWorkspaces,
  localWorkspace,
  onSelectLocalWorkspace,
  onCreateLocalWorkspace,
  onRenameLocalWorkspace,
  onDeleteLocalWorkspace,
  leftOpen,
  setLeftOpen,
  onOpenSettings,
}: MeetingSidebarProps) {
  return (
    <aside className={`left-sidebar ${leftOpen ? 'open' : 'collapsed'}`}>
      <div className="left-sidebar-header">
        <button className="icon-btn" onClick={onOpenSettings} title="设置">
          <Settings size={18} />
        </button>
        <div className="left-title">本地空间</div>
        <button
          className="icon-btn collapse-toggle"
          onClick={() => setLeftOpen(!leftOpen)}
          title={leftOpen ? '收起' : '展开'}
        >
          {leftOpen ? <ChevronLeft size={18} /> : <ChevronRight size={18} />}
        </button>
      </div>

      {leftOpen && (
        <div className="sidebar-content">
          <VoiceInputStatsPanel />
          <LocalWorkspaceSection
            workspaces={localWorkspaces}
            selectedWorkspaceId={localWorkspace.id}
            onSelect={onSelectLocalWorkspace}
            onCreate={onCreateLocalWorkspace}
            onRename={onRenameLocalWorkspace}
            onDelete={onDeleteLocalWorkspace}
          />
        </div>
      )}
    </aside>
  );
}
