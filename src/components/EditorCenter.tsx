import { useEffect, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Mic, Square, RefreshCw, List, Save, Plus, Code, Eye } from 'lucide-react';
import BlockNoteEditorWithSource, { EditorMode } from './BlockNoteEditorWithSource';
import './EditorCenter.css';
import { LocalWorkspace, NoteDoc, RecordingInfo } from '../types';
import { useAudioService } from '../hooks/useServices';

interface Props {
  localWorkspace: LocalWorkspace | null;
  note: NoteDoc | null;
  onNewNote: () => void | Promise<void>;
  onDeleteNote: () => void;
  onChangeNoteContent: (content: string) => void;
  onRecordingSaved: (savedPath: string) => Promise<void>;

  onError: (msg: string) => void;
  onClearError: () => void;
  darkMode: boolean;
}

export default function EditorCenter({
  localWorkspace,
  note,
  onNewNote,
  onDeleteNote: _onDeleteNote,
  onChangeNoteContent,
  onRecordingSaved,
  onError,
  onClearError,
  darkMode,
}: Props) {
  const audioService = useAudioService();

  const [isRecording, setIsRecording] = useState(false);
  const [duration, setDuration] = useState(0);
  const [systemAudioAvailable, setSystemAudioAvailable] = useState<boolean>(false);
  const [isInitializing, setIsInitializing] = useState<boolean>(true);

  // UI state
  const [outlineVisible, setOutlineVisible] = useState(false);

  // Editor mode state
  const [editorMode, setEditorMode] = useState<EditorMode>('wysiwyg');

  // Toggle editor mode
  const toggleEditorMode = useCallback(() => {
    setEditorMode(prev => prev === 'wysiwyg' ? 'source' : 'wysiwyg');
  }, []);

  const formatTime = (seconds: number): string => {
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
  };

  useEffect(() => {
    const initialize = async () => {
      setIsInitializing(true);
      try {
        const info = await invoke<RecordingInfo>('get_recording_info');
        setSystemAudioAvailable(info.system_audio_available);
        await audioService.requestMicrophonePermission();
        onClearError();
      } catch (error) {
        onError('初始化音频设备失败');
        setSystemAudioAvailable(false);
      } finally {
        setIsInitializing(false);
      }
    };
    initialize();
  }, [audioService, onClearError, onError]);

  useEffect(() => {
    const timer = setInterval(async () => {
      try {
        const info = await invoke<RecordingInfo>('get_recording_info');
        setIsRecording(info.is_recording);
        setDuration(info.duration);
        setSystemAudioAvailable(info.system_audio_available);
      } catch {
        setIsRecording(false);
      }
    }, 1000); // Reduced from 200ms to 1000ms to prevent UI freezing
    return () => clearInterval(timer);
  }, []);

  const startRecording = async () => {
    onClearError();
    if (isInitializing) {
      onError('设备初始化中，请稍后...');
      return;
    }
    try {
      const info = await invoke<RecordingInfo>('get_recording_info');
      setSystemAudioAvailable(info.system_audio_available);
      await audioService.startRecording();
    } catch (error) {
      let errorMessage = '录音开始失败';
      if (typeof error === 'string') {
        if (error.includes('Recording already in progress')) errorMessage = '录音已在进行中';
        else if (error.includes('No default input device')) errorMessage = '未找到麦克风设备';
        else if (error.includes('Permission')) errorMessage = '麦克风权限被拒绝';
        else errorMessage = `录音失败: ${error}`;
      }
      onError(errorMessage);
      setIsRecording(false);
      setDuration(0);
    }
  };

  const stopRecording = async () => {
    try {
      if (!localWorkspace?.folderName) {
        throw new Error('当前本地空间不存在');
      }
      const savePath = await invoke<string>('get_workspace_recording_save_path', {
        workspaceFolder: localWorkspace.folderName,
      });
      await audioService.stopRecording({ save_path: savePath });
      await onRecordingSaved(savePath);
      setIsRecording(false);
    } catch (error) {
      let errorMessage = '停止录音失败';
      if (typeof error === 'string') {
        if (error.includes('No audio data captured')) errorMessage = '未捕获到音频数据';
        else if (error.includes('Failed to create save directory')) errorMessage = '无法创建保存目录';
        else if (error.includes('Failed to create WAV')) errorMessage = '无法保存音频文件';
        else errorMessage = `停止失败: ${error}`;
      }
      onError(errorMessage);
      setIsRecording(false);
      setDuration(0);
    }
  };

  return (
    <main className="center-editor">
      <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
        <div className="center-header">
          <div className="record-toolbar">
            <button
              className={`toolbar-icon-btn record-btn ${isRecording ? 'recording' : ''} ${isInitializing ? 'disabled' : ''}`}
              onClick={isRecording ? stopRecording : startRecording}
              title={isRecording ? '停止录音' : (systemAudioAvailable ? '开始混合录音' : '开始录音')}
              disabled={isInitializing}
            >
              {isInitializing ? (
                <RefreshCw size={14} className="spinning" />
              ) : isRecording ? (
                <Square size={14} />
              ) : (
                <Mic size={14} />
              )}
              {isRecording && <span className="record-indicator" />}
            </button>
            <div className="duration-display" title="录音时长">{formatTime(duration)}</div>
          </div>
          <div className="note-actions" style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
            {/* Editor mode toggle */}
            <button
              className="toolbar-icon-btn"
              title={editorMode === 'wysiwyg' ? '切换到源码模式' : '切换到编辑模式'}
              onClick={toggleEditorMode}
              disabled={!note}
            >
              {editorMode === 'wysiwyg' ? <Code size={14} /> : <Eye size={14} />}
            </button>
            <div style={{ width: '1px', height: '20px', backgroundColor: '#e5e7eb' }} />
            <button className="toolbar-icon-btn" title={outlineVisible ? '隐藏大纲' : '显示大纲'} onClick={() => setOutlineVisible(v => !v)} disabled={!note}>
              <List size={14} />
            </button>
            <button className="toolbar-icon-btn" title="自动保存已启用" disabled={true}>
              <Save size={14} />
            </button>
            <button className="toolbar-icon-btn" title="新建笔记" onClick={() => void onNewNote()}>
              <Plus size={14} />
            </button>
          </div>
        </div>

        <div className="note-toolbar" style={{ justifyContent: 'space-between' }}>
          <div style={{ color: '#6b7280', fontSize: '0.9rem' }}>
            {localWorkspace ? `工作区：${localWorkspace.title}` : '本地工作区'}
            {note ? `　|　笔记：${note.title || '未命名笔记'}` : ''}
          </div>
        </div>
        {!note ? (
          <div className="center-empty-state">
            <p style={{ fontSize: '0.875rem', color: '#9ca3af', marginTop: '0.5rem' }}>
              点击右侧列表选择笔记，或点击上方"新建笔记"按钮创建
            </p>
          </div>
        ) : (
          <div className={`editor-container ${outlineVisible ? 'outline-visible' : ''}`}>
            <BlockNoteEditorWithSource
              key={`${localWorkspace?.id || 'local'}:${note.id}`}
              value={note.content || ''}
              onChange={onChangeNoteContent}
              noteId={note.id}
              mode={editorMode}
              darkMode={darkMode}
            />
          </div>
        )}
      </div>
    </main>
  );
}
