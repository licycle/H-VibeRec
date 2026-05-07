import { createContext, useContext, useState, useCallback, useEffect, ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { LocalWorkspace } from '../types';
import {
  createLocalWorkspace,
  deleteLocalWorkspace,
  getLocalWorkspace,
  loadLocalWorkspaces,
  setActiveLocalWorkspaceId,
  syncLocalWorkspacesFromFolders,
  updateWorkspaceTitle,
} from '../lib/workspace';

interface WorkspaceContextType {
  localWorkspaces: LocalWorkspace[];
  localWorkspace: LocalWorkspace;
  switchToLocal: (workspaceId?: string) => void;
  createLocalSpace: (title: string) => LocalWorkspace;
  renameLocalSpace: (workspaceId: string, title: string) => LocalWorkspace;
  removeLocalSpace: (workspaceId: string) => void;
  refreshLocalWorkspaceDirs: (workspaceId?: string) => Promise<void>;
}

const WorkspaceContext = createContext<WorkspaceContextType | undefined>(undefined);

export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const [localWorkspaces, setLocalWorkspaces] = useState<LocalWorkspace[]>(loadLocalWorkspaces());
  const [localWorkspace, setLocalWorkspace] = useState<LocalWorkspace>(getLocalWorkspace());

  const reloadLocalWorkspaces = useCallback((workspaceId?: string) => {
    const workspaces = loadLocalWorkspaces();
    setLocalWorkspaces(workspaces);
    setLocalWorkspace(getLocalWorkspace(workspaceId));
  }, []);

  const refreshLocalWorkspaceDirs = useCallback(async (workspaceId?: string) => {
    const current = getLocalWorkspace(workspaceId);
    await invoke('ensure_workspace_dir', { workspaceFolder: current.folderName });
    const folders = await invoke<string[]>('list_workspace_dirs');
    const workspaces = syncLocalWorkspacesFromFolders(folders);
    setLocalWorkspaces(workspaces);
    setLocalWorkspace(getLocalWorkspace(workspaceId));
  }, []);

  useEffect(() => {
    void refreshLocalWorkspaceDirs();
  }, [refreshLocalWorkspaceDirs]);

  const switchToLocal = useCallback((workspaceId?: string) => {
    if (workspaceId) {
      setActiveLocalWorkspaceId(workspaceId);
    }
    reloadLocalWorkspaces(workspaceId);
  }, [reloadLocalWorkspaces]);

  const createLocalSpace = useCallback((title: string) => {
    const workspace = createLocalWorkspace(title);
    reloadLocalWorkspaces(workspace.id);
    void invoke('ensure_workspace_dir', { workspaceFolder: workspace.folderName })
      .then(() => refreshLocalWorkspaceDirs(workspace.id))
      .catch(error => console.error('Failed to create workspace folder', error));
    return workspace;
  }, [refreshLocalWorkspaceDirs, reloadLocalWorkspaces]);

  const renameLocalSpace = useCallback((workspaceId: string, title: string) => {
    const workspace = updateWorkspaceTitle(workspaceId, title);
    reloadLocalWorkspaces(workspaceId);
    return workspace;
  }, [reloadLocalWorkspaces]);

  const removeLocalSpace = useCallback((workspaceId: string) => {
    deleteLocalWorkspace(workspaceId);
    reloadLocalWorkspaces();
    void refreshLocalWorkspaceDirs();
  }, [refreshLocalWorkspaceDirs, reloadLocalWorkspaces]);

  return (
    <WorkspaceContext.Provider
      value={{
        localWorkspaces,
        localWorkspace,
        switchToLocal,
        createLocalSpace,
        renameLocalSpace,
        removeLocalSpace,
        refreshLocalWorkspaceDirs,
      }}
    >
      {children}
    </WorkspaceContext.Provider>
  );
}

export function useWorkspace() {
  const context = useContext(WorkspaceContext);
  if (context === undefined) {
    throw new Error('useWorkspace must be used within a WorkspaceProvider');
  }
  return context;
}
