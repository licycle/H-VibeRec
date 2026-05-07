import { useState, useCallback, useRef, useMemo, useEffect } from 'react';
import { save } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { NoteDoc, Recording } from '../types';
import {
  createWorkspaceNote,
  saveWorkspaceNote,
  deleteWorkspaceNote,
  listWorkspaceNotes,
  getActiveLocalWorkspaceId,
  setActiveLocalWorkspaceId,
  getLocalWorkspace,
} from '../lib/workspace';
import { confirmLocalAction } from '../lib/confirm';
import { useFileService } from './useServices';

/**
 * useLocalWorkspace Hook
 * 管理本地工作区的笔记和录音
 */
export function useLocalWorkspace() {
  const fileService = useFileService();
  const [workspaceId, setWorkspaceId] = useState<string>(() => getActiveLocalWorkspaceId());
  const workspaceIdRef = useRef<string>(workspaceId);

  // Notes state
  const [notes, setNotes] = useState<NoteDoc[]>([]);
  const notesRef = useRef<NoteDoc[]>([]);
  const [activeNoteId, setActiveNoteId] = useState<string | null>(null);
  const activeNoteIdRef = useRef<string | null>(null);
  const activeNote = useMemo(
    () => notes.find(n => n.id === activeNoteId) || null,
    [notes, activeNoteId]
  );

  // Recordings state
  const [recordings, setRecordings] = useState<Recording[]>([]);
  const [workspaceRecordingIds, setWorkspaceRecordingIds] = useState<string[]>([]);

  // Autosave refs
  const latestContentRef = useRef<string>('');
  const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const notesRefreshSeqRef = useRef<number>(0);
  const recordingsRefreshSeqRef = useRef<number>(0);

  useEffect(() => {
    workspaceIdRef.current = workspaceId;
  }, [workspaceId]);

  useEffect(() => {
    notesRef.current = notes;
  }, [notes]);

  useEffect(() => {
    activeNoteIdRef.current = activeNoteId;
  }, [activeNoteId]);

  // Sync active note content reference when note changes (only on note switch, not on every edit)
  useEffect(() => {
    latestContentRef.current = activeNote?.content || '';
  }, [activeNote?.id, workspaceId]);

  // ====================  Notes Management ====================

  const refreshNotes = useCallback(
    async (
      targetWorkspaceId = workspaceIdRef.current,
      preferredNoteId?: string | null
    ): Promise<NoteDoc[]> => {
      const requestSeq = ++notesRefreshSeqRef.current;
      try {
        const ns = await listWorkspaceNotes(targetWorkspaceId);
        if (
          workspaceIdRef.current !== targetWorkspaceId ||
          requestSeq !== notesRefreshSeqRef.current
        ) {
          return ns;
        }

        const candidate = preferredNoteId ?? activeNoteIdRef.current;
        const nextActiveNoteId = candidate && ns.some(n => n.id === candidate)
          ? candidate
          : ns[0]?.id || null;

        notesRef.current = ns;
        activeNoteIdRef.current = nextActiveNoteId;
        latestContentRef.current = ns.find(n => n.id === nextActiveNoteId)?.content || '';
        setNotes(ns);
        setActiveNoteId(nextActiveNoteId);
        return ns;
      } catch (error) {
        console.error('Failed to refresh notes', error);
        if (
          workspaceIdRef.current === targetWorkspaceId &&
          requestSeq === notesRefreshSeqRef.current
        ) {
          notesRef.current = [];
          activeNoteIdRef.current = null;
          latestContentRef.current = '';
          setNotes([]);
          setActiveNoteId(null);
        }
        return [];
      }
    },
    []
  );

  useEffect(() => {
    void refreshNotes(workspaceIdRef.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const forceSaveActiveNoteNow = useCallback(async (
    targetWorkspaceId = workspaceIdRef.current
  ): Promise<NoteDoc | null> => {
    const noteId = activeNoteIdRef.current;
    if (!noteId) {
      if (saveTimeoutRef.current) {
        clearTimeout(saveTimeoutRef.current);
        saveTimeoutRef.current = null;
      }
      return null;
    }
    try {
      if (saveTimeoutRef.current) {
        clearTimeout(saveTimeoutRef.current);
        saveTimeoutRef.current = null;
      }

      const current = notesRef.current.find(n => n.id === noteId);
      if (!current) return null;

      const content = latestContentRef.current ?? current.content ?? '';
      const toSave: NoteDoc = { ...current, content, updated: new Date().toISOString() };

      if (workspaceIdRef.current === targetWorkspaceId) {
        notesRef.current = notesRef.current.map(n => (n.id === toSave.id ? toSave : n));
        setNotes(prev => prev.map(n => (n.id === toSave.id ? toSave : n)));
      }

      await saveWorkspaceNote(toSave, targetWorkspaceId);
      return toSave;
    } catch (e) {
      console.error('forceSaveActiveNoteNow failed:', e);
      return null;
    }
  }, []);

  const refreshRecordings = useCallback(async (
    targetWorkspaceId = workspaceIdRef.current
  ): Promise<Recording[]> => {
    const requestSeq = ++recordingsRefreshSeqRef.current;
    try {
      const workspace = getLocalWorkspace(targetWorkspaceId);
      const list = await invoke<Recording[]>('list_workspace_recordings', {
        workspaceFolder: workspace.folderName,
      });
      if (
        workspaceIdRef.current !== targetWorkspaceId ||
        requestSeq !== recordingsRefreshSeqRef.current
      ) {
        return list;
      }
      setWorkspaceRecordingIds(list.map(recording => recording.id));
      setRecordings(list);
      return list;
    } catch (e) {
      console.error('Failed to load recordings', e);
      if (
        workspaceIdRef.current === targetWorkspaceId &&
        requestSeq === recordingsRefreshSeqRef.current
      ) {
        setRecordings([]);
        setWorkspaceRecordingIds([]);
      }
      return [];
    }
  }, []);

  useEffect(() => {
    void refreshRecordings(workspaceIdRef.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const switchWorkspace = useCallback(async (
    nextWorkspaceId: string,
    options?: { skipSave?: boolean; preferredNoteId?: string | null }
  ) => {
    const previousWorkspaceId = workspaceIdRef.current;
    notesRefreshSeqRef.current += 1;
    recordingsRefreshSeqRef.current += 1;
    if (!options?.skipSave) {
      await forceSaveActiveNoteNow(previousWorkspaceId);
    }

    setActiveLocalWorkspaceId(nextWorkspaceId);
    workspaceIdRef.current = nextWorkspaceId;
    setWorkspaceId(nextWorkspaceId);

    notesRef.current = [];
    activeNoteIdRef.current = null;
    latestContentRef.current = '';
    setNotes([]);
    setActiveNoteId(null);

    setRecordings([]);
    setWorkspaceRecordingIds([]);

    void refreshNotes(nextWorkspaceId, options?.preferredNoteId);
    void refreshRecordings(nextWorkspaceId);
  }, [forceSaveActiveNoteNow, refreshNotes, refreshRecordings]);

  const newNote = useCallback(async () => {
    const targetWorkspaceId = workspaceIdRef.current;
    await forceSaveActiveNoteNow(targetWorkspaceId);
    const n = await createWorkspaceNote('新笔记', '', targetWorkspaceId);
    await refreshNotes(targetWorkspaceId, n.id);
    activeNoteIdRef.current = n.id;
    setActiveNoteId(n.id);
  }, [forceSaveActiveNoteNow, refreshNotes]);

  const deleteActiveNote = useCallback(() => {
    const targetWorkspaceId = workspaceIdRef.current;
    const targetActiveNoteId = activeNoteIdRef.current;
    if (!targetActiveNoteId) return;

    const targetNote = notesRef.current.find(n => n.id === targetActiveNoteId);
    const noteTitle = targetNote?.title || '未命名笔记';

    void confirmLocalAction(`确定要删除"${noteTitle}"吗？此操作不可撤销。`, {
      title: '确认删除',
      confirmLabel: '删除',
      destructive: true,
    }).then(accepted => {
      if (!accepted) return;

      if (saveTimeoutRef.current) {
        clearTimeout(saveTimeoutRef.current);
        saveTimeoutRef.current = null;
      }
      void deleteWorkspaceNote(targetActiveNoteId, targetWorkspaceId)
        .then(() => refreshNotes(targetWorkspaceId))
        .catch(error => {
          console.error('Failed to delete note:', error);
        });
    });
  }, [refreshNotes]);

  const renameNoteById = useCallback((noteId: string, title: string) => {
    const targetWorkspaceId = workspaceIdRef.current;
    setNotes((prev: NoteDoc[]) => {
      const updated = prev.map(n => (n.id === noteId ? { ...n, title } : n));
      notesRef.current = updated;
      const target = updated.find(n => n.id === noteId);
      if (target) {
        void saveWorkspaceNote({ ...target, updated: new Date().toISOString() }, targetWorkspaceId).catch(error => {
          console.error('Failed to rename note:', error);
        });
      }
      return updated;
    });
  }, []);

  const deleteNoteFromList = useCallback(
    (note: NoteDoc) => {
      const targetWorkspaceId = workspaceIdRef.current;
      const targetActiveNoteId = activeNoteIdRef.current;
      void confirmLocalAction(`确定要删除"${note.title || '未命名笔记'}"吗？此操作不可撤销。`, {
        title: '确认删除',
        confirmLabel: '删除',
        destructive: true,
      }).then(accepted => {
        if (!accepted) return;

        if (targetActiveNoteId === note.id && saveTimeoutRef.current) {
          clearTimeout(saveTimeoutRef.current);
          saveTimeoutRef.current = null;
        }
        void deleteWorkspaceNote(note.id, targetWorkspaceId)
          .then(() => refreshNotes(targetWorkspaceId, targetActiveNoteId === note.id ? null : targetActiveNoteId))
          .catch(error => {
            console.error('Failed to delete note:', error);
          });
      });
    },
    [refreshNotes]
  );

  const handleNotesImported = useCallback(async (importedNotes: NoteDoc[]) => {
    const targetWorkspaceId = workspaceIdRef.current;
    const savedNotes: NoteDoc[] = [];
    for (const note of importedNotes) {
      savedNotes.push(await createWorkspaceNote(note.title, note.content, targetWorkspaceId));
    }
    if (importedNotes.length > 0) {
      await refreshNotes(targetWorkspaceId, savedNotes[0]?.id);
    }
  }, [refreshNotes]);

  const createNoteWithContent = useCallback(async (title: string, content: string) => {
    const targetWorkspaceId = workspaceIdRef.current;
    await forceSaveActiveNoteNow(targetWorkspaceId);
    const note = await createWorkspaceNote(title, content, targetWorkspaceId);
    await refreshNotes(targetWorkspaceId, note.id);
    activeNoteIdRef.current = note.id;
    setActiveNoteId(note.id);
    return note;
  }, [forceSaveActiveNoteNow, refreshNotes]);

  const handleRefreshNotes = useCallback(() => {
    void refreshNotes(workspaceIdRef.current);
  }, [refreshNotes]);

  const handleSelectNote = useCallback(
    async (id: string) => {
      const targetWorkspaceId = workspaceIdRef.current;
      try {
        await forceSaveActiveNoteNow(targetWorkspaceId);
      } catch (err) {
        console.error('forceSaveActiveNoteNow failed:', err);
      }
      activeNoteIdRef.current = id;
      setActiveNoteId(id);
    },
    [forceSaveActiveNoteNow]
  );

  const exportNoteAsMarkdown = useCallback(
    async (note: NoteDoc) => {
      try {
        const content = note.content || '# ' + (note.title || '未命名笔记') + '\n\n内容为空';
        const fileName = `${(note.title || '未命名笔记').replace(/[/\\?%*:|"<>]/g, '-')}.md`;

        const filePath = await save({
          title: '导出笔记',
          defaultPath: fileName,
          filters: [
            {
              name: 'Markdown 文件',
              extensions: ['md'],
            },
          ],
        });

        if (filePath) {
          await fileService.saveTextFile(content, filePath);
          console.log('导出成功:', filePath);
        }
      } catch (error) {
        console.error('导出失败:', error);
        throw new Error('导出Markdown文件失败');
      }
    },
    [fileService]
  );

  const changeNoteContent = useCallback(
    (content: string) => {
      const noteId = activeNoteIdRef.current;
      if (!noteId) return;

      const targetWorkspaceId = workspaceIdRef.current;

      // Only update content reference, don't trigger state update yet
      latestContentRef.current = content;

      // Debounced save
      if (saveTimeoutRef.current) {
        clearTimeout(saveTimeoutRef.current);
      }

      saveTimeoutRef.current = setTimeout(() => {
        if (workspaceIdRef.current !== targetWorkspaceId) {
          saveTimeoutRef.current = null;
          return;
        }

        // Only update state and save when debounce completes
        setNotes(prevNotes => {
          const updatedNotes = prevNotes.map(note => {
            if (note.id === noteId) {
              const updatedNote = { ...note, content, updated: new Date().toISOString() };
              void saveWorkspaceNote(updatedNote, targetWorkspaceId).catch(error => {
                console.error('Failed to save note content:', error);
              });
              return updatedNote;
            }
            return note;
          });
          notesRef.current = updatedNotes;
          return updatedNotes;
        });
        saveTimeoutRef.current = null;
      }, 500);
    },
    []
  );

  // ==================== Recordings Management ====================

  const onRecordingSaved = useCallback(
    async (savedPath: string) => {
      const targetWorkspaceId = workspaceIdRef.current;
      const list = await refreshRecordings(targetWorkspaceId);
      if (workspaceIdRef.current !== targetWorkspaceId) return;
      const rec = list.find(r => r.path === savedPath);
      if (rec) {
        setWorkspaceRecordingIds(list.map(recording => recording.id));
      }
    },
    [refreshRecordings]
  );

  const handleRecordingRenamed = useCallback((oldId: string, newRecording: Recording) => {
    setWorkspaceRecordingIds(prev => {
      if (!prev.includes(oldId)) return prev;
      return prev.map(id => (id === oldId ? newRecording.id : id));
    });
  }, []);

  const handleRecordingDeleted = useCallback((recordingId: string) => {
    setWorkspaceRecordingIds(prev => prev.filter(id => id !== recordingId));
    setRecordings(prev => prev.filter(recording => recording.id !== recordingId));
  }, []);

  const handleRecordingsImported = useCallback((recordingIds: string[]) => {
    setWorkspaceRecordingIds(prev => [...new Set([...prev, ...recordingIds])]);
  }, []);

  return {
    workspaceId,
    switchWorkspace,
    // Notes
    notes,
    activeNoteId,
    activeNote,
    newNote,
    deleteActiveNote,
    renameNoteById,
    deleteNoteFromList,
    handleNotesImported,
    createNoteWithContent,
    refreshNotes,
    handleRefreshNotes,
    handleSelectNote,
    exportNoteAsMarkdown,
    changeNoteContent,
    forceSaveActiveNoteNow,
    // Recordings
    recordings,
    workspaceRecordingIds,
    refreshRecordings,
    onRecordingSaved,
    handleRecordingRenamed,
    handleRecordingDeleted,
    handleRecordingsImported,
  };
}
