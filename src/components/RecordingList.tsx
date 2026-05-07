import { useState, useEffect, useMemo } from 'react';
import { save, open } from '@tauri-apps/plugin-dialog';
import { Play, Download, Trash2, HardDrive, Check, X, MoreVertical, RefreshCw, Import, FileText, Zap as Bolt, Loader2 } from 'lucide-react';
import { Recording } from '../types';
import { useAudioService } from '../hooks/useServices';
import { invoke } from '@tauri-apps/api/core';
import { confirmLocalAction } from '../lib/confirm';
import {
  LocalRecording,
  LocalQueueJob,
  SummaryTemplate,
} from '../appTypes';
import SummaryTemplatePicker from './SummaryTemplatePicker';
import './RecordingList.css';

interface Props {
  recordings: Recording[];
  meetingRecordingIds: string[];
  workspaceFolder?: string;
  workspaceTitle?: string;
  refreshRecordings: () => Promise<Recording[]>;
  onError: (msg: string) => void;
  onClearError: () => void;
  onRecordingRenamed?: (oldId: string, newRecording: Recording) => void;
  onRecordingDeleted?: (recordingId: string) => void;
  onRecordingsImported?: (recordingIds: string[]) => void;
  onTranscriptCreated?: (title: string, content: string) => unknown | Promise<unknown>;
  onWorkspaceSummaryCreated?: (title: string, content: string) => unknown | Promise<unknown>;
}

type SummaryPickerTarget =
  | { mode: 'single'; recording: Recording }
  | { mode: 'all' }
  | null;

export default function RecordingList({
  recordings,
  meetingRecordingIds,
  workspaceFolder,
  workspaceTitle = '本地工作区',
  refreshRecordings,
  onError,
  onClearError,
  onRecordingRenamed,
  onRecordingDeleted,
  onRecordingsImported,
}: Props) {
  // 服务注入
  const audioService = useAudioService();

  const [editingRecordingId, setEditingRecordingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState<string>('');
  const [activeMenuId, setActiveMenuId] = useState<string | null>(null);
  const [isImporting, setIsImporting] = useState<boolean>(false);
  const [playingAudio, setPlayingAudio] = useState<{ url: string; name: string; path: string } | null>(null);
  const [localStatuses, setLocalStatuses] = useState<LocalRecording[]>([]);
  const [summaryTemplates, setSummaryTemplates] = useState<SummaryTemplate[]>([]);
  const [templatesLoading, setTemplatesLoading] = useState<boolean>(false);
  const [summaryPickerTarget, setSummaryPickerTarget] = useState<SummaryPickerTarget>(null);
  const [busyRecordingId, setBusyRecordingId] = useState<string | null>(null);
  const [bulkAction, setBulkAction] = useState<'transcribe' | 'pipeline' | null>(null);
  const [bulkProgress, setBulkProgress] = useState<{ done: number; total: number } | null>(null);

  const recs = recordings.filter(r => meetingRecordingIds.includes(r.id));
  const visibleRecordingIds = useMemo(() => new Set(meetingRecordingIds), [meetingRecordingIds]);
  const statusById = useMemo(() => {
    const map = new Map<string, LocalRecording>();
    localStatuses.forEach(status => {
      const byPath = recordings.find(recording => recording.path === status.original_audio_path);
      if (visibleRecordingIds.has(status.id) || byPath) {
        map.set(status.id, status);
      }
      if (byPath) {
        map.set(byPath.id, status);
      }
    });
    return map;
  }, [localStatuses, recordings, visibleRecordingIds]);
  const statusForRecording = (recording: Recording) =>
    statusById.get(recording.id) ||
    localStatuses.find(status => status.original_audio_path === recording.path) ||
    null;

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (activeMenuId && !(event.target as Element).closest('.actions-menu-container')) {
        setActiveMenuId(null);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [activeMenuId]);

  useEffect(() => {
    void refreshLocalStatuses();
    void loadSummaryTemplates();
  }, [recordings.length, meetingRecordingIds.join('|')]);

  const refreshLocalStatuses = async () => {
    try {
      const statuses = await invoke<LocalRecording[]>('list_recordings_with_status');
      setLocalStatuses(statuses);
      return statuses;
    } catch (error) {
      console.warn('Failed to load local recording statuses', error);
      return [];
    }
  };

  const loadSummaryTemplates = async () => {
    setTemplatesLoading(true);
    try {
      const templates = await invoke<SummaryTemplate[]>('list_summary_templates');
      setSummaryTemplates(templates);
    } catch (error) {
      console.warn('Failed to load summary templates', error);
      onError(typeof error === 'string' ? error : '加载总结模板失败');
    } finally {
      setTemplatesLoading(false);
    }
  };

  const registerRecordingForLocalPipeline = async (recording: Recording) => {
    const registered = await invoke<LocalRecording>('register_recording', {
      filePath: recording.path,
      title: recording.name.replace(/\.[^/.]+$/, ''),
    });
    await refreshLocalStatuses();
    return registered;
  };

  const transcribeRecording = async (recording: Recording, quiet = false) => {
    if (!quiet) setBusyRecordingId(recording.id);
    setActiveMenuId(null);
    onClearError();
    try {
      const registered = await registerRecordingForLocalPipeline(recording);
      await invoke<LocalQueueJob>('enqueue_transcription', {
        recordingId: registered.id,
        workspaceFolder,
        nextSummaryTemplateId: null,
      });
      await refreshLocalStatuses();
      await refreshRecordings();
    } catch (error) {
      onError(typeof error === 'string' ? error : '转录入队失败');
      throw error;
    } finally {
      if (!quiet) setBusyRecordingId(null);
    }
  };

  const summarizeRecording = async (
    recording: Recording,
    template: SummaryTemplate,
    quiet = false
  ) => {
    if (!quiet) setBusyRecordingId(recording.id);
    setActiveMenuId(null);
    onClearError();
    try {
      const currentStatus = statusForRecording(recording);
      if (currentStatus?.latest_transcript_id) {
        await invoke<LocalQueueJob>('enqueue_summary', {
          transcriptId: currentStatus.latest_transcript_id,
          templateId: template.id,
          workspaceFolder,
        });
      } else {
        const registered = currentStatus || await registerRecordingForLocalPipeline(recording);
        await invoke<LocalQueueJob>('enqueue_transcription', {
          recordingId: registered.id,
          workspaceFolder,
          nextSummaryTemplateId: template.id,
        });
      }
      await refreshLocalStatuses();
      await refreshRecordings();
    } catch (error) {
      onError(error instanceof Error ? error.message : typeof error === 'string' ? error : '总结入队失败');
      throw error;
    } finally {
      if (!quiet) setBusyRecordingId(null);
    }
  };

  const transcribeAll = async () => {
    await runBulkAction('transcribe', async recording => {
      await transcribeRecording(recording, true);
    });
  };

  const summarizeAllRecordings = async (template: SummaryTemplate) => {
    await runBulkAction('pipeline', async recording => {
      await summarizeRecording(recording, template, true);
    });
  };

  const runBulkAction = async (
    action: 'transcribe' | 'pipeline',
    handler: (recording: Recording) => Promise<void>
  ) => {
    if (!recs.length || bulkAction) return;
    setBulkAction(action);
    setBusyRecordingId(null);
    setActiveMenuId(null);
    setBulkProgress({ done: 0, total: recs.length });
    onClearError();
    try {
      for (let index = 0; index < recs.length; index += 1) {
        const recording = recs[index];
        setBusyRecordingId(recording.id);
        await handler(recording);
        setBulkProgress({ done: index + 1, total: recs.length });
      }
    } catch (error) {
      const label = action === 'transcribe' ? '批量转录' : '批量一键总结';
      onError(error instanceof Error ? error.message : typeof error === 'string' ? error : `${label}失败`);
    } finally {
      setBusyRecordingId(null);
      setBulkAction(null);
      window.setTimeout(() => setBulkProgress(null), 1200);
    }
  };

  const openSummaryPicker = async (target: Exclude<SummaryPickerTarget, null>) => {
    setActiveMenuId(null);
    setSummaryPickerTarget(target);
    if (!summaryTemplates.length) {
      await loadSummaryTemplates();
    }
  };

  const handleSummaryTemplatePicked = async (template: SummaryTemplate) => {
    const target = summaryPickerTarget;
    setSummaryPickerTarget(null);
    if (!target) return;
    try {
      if (target.mode === 'single') {
        await summarizeRecording(target.recording, template);
        return;
      }
      await summarizeAllRecordings(template);
    } catch {
      // The called workflow already reports a user-facing error.
    }
  };

  const playRecording = async (recording: Recording) => {
    try {
      // 使用浏览器内置播放器，而不是外部程序
      const audioUrl = audioService.getLocalAudioUrl(recording.path);
      setPlayingAudio({ url: audioUrl, name: recording.name, path: recording.path });
      onClearError();
    } catch (error) {
      let errorMessage = '播放失败';
      if (typeof error === 'string') {
        if (error.includes('not found')) errorMessage = '音频文件不存在';
        else if (error.includes('Permission')) errorMessage = '无权访问文件';
        else errorMessage = `播放失败: ${error}`;
      }
      onError(errorMessage);
    }
  };

  const exportRecording = async (recording: Recording) => {
    try {
      const fileExtension = recording.name.split('.').pop() || 'flac';
      const baseFileName = recording.name.replace(/\.(wav|flac)$/, '');
      const defaultFileName = baseFileName + '_exported.' + fileExtension;
      const targetPath = await save({
        title: '导出录音',
        defaultPath: defaultFileName,
        filters: [
          { name: 'FLAC 音频文件', extensions: ['flac'] },
          { name: 'WAV 音频文件', extensions: ['wav'] },
          { name: '所有音频文件', extensions: ['flac', 'wav'] },
        ],
      });
      if (targetPath) {
        await audioService.exportAudioFile(recording.path, targetPath);
        onClearError();
      }
    } catch (error) {
      let errorMessage = '导出失败';
      if (typeof error === 'string') {
        if (error.includes('not found')) errorMessage = '源文件不存在';
        else if (error.includes('Permission')) errorMessage = '无权访问文件目录';
        else if (error.includes('create')) errorMessage = '无法创建目标文件';
        else errorMessage = `导出失败: ${error}`;
      }
      onError(errorMessage);
    }
  };

  const isMissingFileError = (error: unknown) => {
    const message = typeof error === 'string' ? error : error instanceof Error ? error.message : '';
    return message.includes('Audio file not found') ||
      message.includes('File not found') ||
      message.includes('Source file not found');
  };

  const deleteRecording = async (recording: Recording) => {
    const confirmed = await confirmLocalAction(`确定要删除 "${recording.name}" 吗？此操作不可撤销。`, {
      title: '确认删除',
      confirmLabel: '删除',
      destructive: true,
    });
    if (!confirmed) return;
    console.info('[RecordingList] delete requested', {
      workspaceFolder,
      id: recording.id,
      name: recording.name,
      path: recording.path,
    });
    try {
      const deleteByPath = async () => {
        try {
          console.info('[RecordingList] deleting by file path', {
            id: recording.id,
            path: recording.path,
          });
          await audioService.deleteAudioFile(recording.path);
          console.info('[RecordingList] delete by file path succeeded', {
            id: recording.id,
            path: recording.path,
          });
        } catch (error) {
          console.error('[RecordingList] delete by file path failed', {
            id: recording.id,
            path: recording.path,
            error,
          });
          if (!isMissingFileError(error)) {
            throw error;
          }
        }
      };

      if (workspaceFolder) {
        try {
          console.info('[RecordingList] deleting by workspace recording id', {
            workspaceFolder,
            id: recording.id,
          });
          await audioService.deleteWorkspaceRecording(workspaceFolder, recording.id);
          console.info('[RecordingList] delete by workspace recording id succeeded', {
            workspaceFolder,
            id: recording.id,
          });
        } catch (error) {
          console.warn('[RecordingList] delete by workspace recording id failed; falling back to file path', {
            workspaceFolder,
            id: recording.id,
            path: recording.path,
            error,
          });
          await deleteByPath();
        }
      } else {
        await deleteByPath();
      }

      if (playingAudio?.path === recording.path) {
        setPlayingAudio(null);
      }
      if (onRecordingDeleted) {
        onRecordingDeleted(recording.id);
      }
      await refreshRecordings();
      console.info('[RecordingList] delete flow finished and recordings refreshed', {
        id: recording.id,
      });
      onClearError();
    } catch (error) {
      console.error('[RecordingList] delete flow failed', {
        id: recording.id,
        path: recording.path,
        error,
      });
      let errorMessage = '删除失败';
      if (typeof error === 'string') {
        if (error.includes('not found')) errorMessage = '文件不存在';
        else if (error.includes('Permission')) errorMessage = '无权删除文件';
        else errorMessage = `删除失败: ${error}`;
      }
      onError(errorMessage);
    }
  };


  const startEditRecording = (recording: Recording) => {
    setEditingRecordingId(recording.id);
    const nameWithoutExtension = recording.name.replace(/\.[^/.]+$/, '');
    setEditingName(nameWithoutExtension);
  };

  const cancelEditRecording = () => {
    setEditingRecordingId(null);
    setEditingName('');
  };

  const saveEditRecording = async (recording: Recording) => {
    if (!editingName.trim()) {
      onError('文件名不能为空');
      return;
    }
    const fileExtension = recording.name.split('.').pop();
    const newFileName = `${editingName.trim()}.${fileExtension}`;
    if (newFileName === recording.name) {
      cancelEditRecording();
      return;
    }
    try {
      const newFilePath = recording.path.replace(recording.name, newFileName);
      await audioService.exportAudioFile(recording.path, newFilePath);
      await audioService.deleteAudioFile(recording.path);
      const updatedRecordings = await refreshRecordings();
      if (onRecordingRenamed) {
        const renamedRecord = updatedRecordings.find(r => r.path === newFilePath);
        if (renamedRecord) {
          onRecordingRenamed(recording.id, renamedRecord);
        }
      }
      cancelEditRecording();
    } catch (error) {
      onError(typeof error === 'string' ? error : '重命名失败');
    }
  };

  const importAudioFiles = async () => {
    try {
      setIsImporting(true);
      onClearError();
      if (!workspaceFolder) {
        throw new Error('当前本地空间不存在');
      }

      // Open file dialog for multiple audio files
      const files = await open({
        title: '导入音频文件',
        multiple: true,
        filters: [{
          name: '音频文件',
          extensions: ['wav', 'flac', 'mp3', 'm4a', 'aac', 'ogg']
        }]
      });

      if (!files || (Array.isArray(files) && files.length === 0)) {
        setIsImporting(false);
        return;
      }

      // Convert to array if single file
      const filePaths = Array.isArray(files) ? files : [files];

      // Call backend to import files and get imported recordings
      const importedRecordings = await invoke<Recording[]>('import_audio_files_to_workspace', {
          filePaths,
          workspaceFolder,
      });

      // Refresh recordings list
      await refreshRecordings();

      // Add imported recordings to workspace
      if (onRecordingsImported && importedRecordings.length > 0) {
        const recordingIds = importedRecordings.map(r => r.id);
        onRecordingsImported(recordingIds);
      }

      onClearError();
    } catch (error) {
      onError(typeof error === 'string' ? error : '导入音频文件失败');
    } finally {
      setIsImporting(false);
    }
  };

  return (
    <div className="right-pane-section">
      {playingAudio && (
        <div className="audio-player-container">
          <div className="audio-player-header">
            <Play size={20} className="audio-player-icon" />
            <span className="audio-player-title">{playingAudio.name}</span>
            <button
              className="audio-player-close"
              onClick={() => setPlayingAudio(null)}
              title="关闭播放器"
            >
              <X size={18} />
            </button>
          </div>
          <audio
            key={playingAudio.url}
            src={playingAudio.url}
            controls
            autoPlay
            className="audio-player"
          />
        </div>
      )}

      <div className="recordings-list">
        <div className="recordings-header">
          <button
            className="refresh-button"
            onClick={() => refreshRecordings()}
            title="刷新列表"
          >
            <RefreshCw size={16} />
          </button>
          <button
            className="import-button"
            onClick={importAudioFiles}
            disabled={isImporting}
            title="导入音频文件"
          >
            <Import size={16} />
            {isImporting ? '导入中...' : '导入音频'}
          </button>
          <button
            className="mini-btn"
            onClick={() => void transcribeAll()}
            disabled={!recs.length || !!busyRecordingId || !!bulkAction}
            title="转录当前工作区全部录音"
          >
            {bulkAction === 'transcribe' ? <Loader2 size={16} className="spinning" /> : <FileText size={16} />}
          </button>
          <button
            className="pipeline-button"
            onClick={() => void openSummaryPicker({ mode: 'all' })}
            disabled={!recs.length || !!busyRecordingId || !!bulkAction}
            title="当前录音栏一键转录+总结"
          >
            {bulkAction === 'pipeline' ? <Loader2 size={16} className="spinning" /> : <Bolt size={16} />}
            一键总结
          </button>
        </div>
        {bulkProgress && (
          <div className="bulk-progress">
            {bulkAction === 'pipeline' ? '正在一键转录+总结' : '正在转录'}：{bulkProgress.done}/{bulkProgress.total}
          </div>
        )}

        {!recs.length ? (
          <div className="empty-state">
            <RefreshCw size={48} />
            <h2>暂无录音</h2>
            <p>点击中间区域的"开始录音"创建，或导入现有音频文件</p>
          </div>
        ) : (
          recs.map((recording) => (
            <div key={recording.id} className="recording-item">
              <div className="recording-main">
                <div className="recording-title-container">
                  {editingRecordingId === recording.id ? (
                    <div className="edit-name-container">
                      <input
                        className="edit-name-input"
                        value={editingName}
                        onChange={(e) => setEditingName(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') { e.preventDefault(); saveEditRecording(recording); }
                          else if (e.key === 'Escape') { cancelEditRecording(); }
                        }}
                        autoFocus
                      />
                      <div className="edit-actions">
                        <button
                          className="mini-btn"
                          onMouseDown={(e) => e.preventDefault()}
                          onClick={() => saveEditRecording(recording)}
                          title="保存"
                        >
                          <Check size={12} />
                        </button>
                        <button
                          className="mini-btn"
                          onMouseDown={(e) => e.preventDefault()}
                          onClick={cancelEditRecording}
                          title="取消"
                        >
                          <X size={12} />
                        </button>
                      </div>
                    </div>
                  ) : (
                    <div className="recording-title">{recording.name}</div>
                  )}

                  <div className="recording-badges">
                    <span className="recording-badge local-badge" title="本地文件">
                      <HardDrive size={12} />
                      本地
                    </span>
                    <span
                      className="recording-badge size-badge"
                      title={`文件大小: ${
                        recording.size > 1024 * 1024
                          ? `${(recording.size / (1024 * 1024)).toFixed(1)}MB`
                          : `${(recording.size / 1024).toFixed(0)}KB`
                      }`}
                    >
                      {recording.size > 1024 * 1024
                        ? `${(recording.size / (1024 * 1024)).toFixed(1)}MB`
                        : `${(recording.size / 1024).toFixed(0)}KB`}
                    </span>
                    <span className={`recording-badge status-badge ${statusTone(statusForRecording(recording)?.transcription_status)}`}>
                      转录：{statusLabel(statusForRecording(recording)?.transcription_status)}
                    </span>
                    <span className={`recording-badge status-badge ${statusTone(statusForRecording(recording)?.summary_status)}`}>
                      总结：{statusLabel(statusForRecording(recording)?.summary_status)}
                    </span>
                  </div>
                </div>
                <div className="recording-time">{recording.created}</div>
              </div>

              {editingRecordingId !== recording.id && (
                <div className="recording-actions">
                  <button className="mini-btn" onClick={() => playRecording(recording)} title="播放录音">
                    <Play size={18} />
                  </button>
                  <button
                    className="mini-btn"
                    onClick={() => void transcribeRecording(recording)}
                    title="转录"
                    disabled={!!bulkAction || busyRecordingId === recording.id}
                  >
                    {busyRecordingId === recording.id ? <Loader2 size={16} className="spinning" /> : <FileText size={16} />}
                  </button>
                  <button
                    className="mini-btn"
                    onClick={() => void openSummaryPicker({ mode: 'single', recording })}
                    title="一键转录+总结"
                    disabled={!!bulkAction || busyRecordingId === recording.id}
                  >
                    {busyRecordingId === recording.id ? <Loader2 size={16} className="spinning" /> : <Bolt size={16} />}
                  </button>

                  <div className="actions-menu-container">
                    <button className="mini-btn" onClick={() => setActiveMenuId(activeMenuId === recording.id ? null : recording.id)} title="更多操作">
                      <MoreVertical size={18} />
                    </button>
                    {activeMenuId === recording.id && (
                      <div className="actions-menu">
                        <button
                          className="menu-item"
                          onClick={() => {
                            void transcribeRecording(recording);
                          }}
                          disabled={!!bulkAction || busyRecordingId === recording.id}
                        >
                          <FileText size={14} /> 转录
                        </button>
                        <button
                          className="menu-item"
                          onClick={() => {
                            void openSummaryPicker({ mode: 'single', recording });
                          }}
                          disabled={!!bulkAction || busyRecordingId === recording.id}
                        >
                          <Bolt size={14} /> 一键总结
                        </button>
                        <button
                          className="menu-item"
                          onClick={() => {
                            startEditRecording(recording);
                            setActiveMenuId(null);
                          }}
                          disabled={editingRecordingId === recording.id}
                        >
                          重命名
                        </button>
                        <button
                          className="menu-item"
                          onClick={() => {
                            exportRecording(recording);
                            setActiveMenuId(null);
                          }}
                        >
                          <Download size={14} /> 导出
                        </button>
                        <button
                          className="menu-item danger"
                          onClick={() => {
                            deleteRecording(recording);
                            setActiveMenuId(null);
                          }}
                        >
                          <Trash2 size={14} /> 删除
                        </button>
                      </div>
                    )}
                  </div>
                </div>
              )}
            </div>
          ))
        )}
      </div>
      <SummaryTemplatePicker
        open={summaryPickerTarget !== null}
        title={summaryPickerTarget?.mode === 'all' ? '选择录音一键总结模板' : '选择单个录音总结模板'}
        description={
          summaryPickerTarget?.mode === 'all'
            ? `对"${workspaceTitle}"录音栏中的全部录音逐个执行转录和总结。`
            : '将先转录该录音，再按所选模板生成总结。'
        }
        templates={summaryTemplates}
        loading={templatesLoading}
        confirmLabel="开始总结"
        onRefresh={loadSummaryTemplates}
        onCancel={() => setSummaryPickerTarget(null)}
        onConfirm={handleSummaryTemplatePicked}
      />
    </div>
  );
}

function statusLabel(status?: string | null) {
  switch (status) {
    case 'succeeded':
    case 'ready':
      return '完成';
    case 'running':
      return '处理中';
    case 'failed':
      return '失败';
    case 'not_started':
    case 'not_downloaded':
    case undefined:
    case null:
      return '未开始';
    default:
      return status;
  }
}

function statusTone(status?: string | null) {
  if (status === 'succeeded' || status === 'ready') return 'ok';
  if (status === 'running') return 'warn';
  if (status === 'failed') return 'bad';
  return 'neutral';
}
