import { invoke } from '@tauri-apps/api/core';
import { convertFileSrc } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  VoiceInputDictationResult,
  VoiceInputPermissionStatus,
  VoiceInputStats,
  VoiceInputStatus,
  VoiceInputStatusEvent,
  VoiceInputWarmupStatusEvent,
} from '../appTypes';

export type VoiceInputPermissionSettingsTarget = 'microphone' | 'accessibility';

// 音频服务接口
export interface AudioService {
  requestMicrophonePermission(): Promise<void>;
  startRecording(): Promise<void>;
  stopRecording(args: { save_path: string }): Promise<void>;
  playAudioFile(filePath: string): Promise<void>;
  getLocalAudioUrl(filePath: string): string; // 新增：获取本地音频的浏览器访问URL
  exportAudioFile(sourcePath: string, targetPath: string): Promise<void>;
  deleteAudioFile(filePath: string): Promise<void>;
  deleteWorkspaceRecording(workspaceFolder: string, recordingId: string): Promise<void>;
}

// 文件服务接口
export interface FileService {
  saveTextFile(content: string, targetPath: string): Promise<void>;
}

export interface VoiceInputService {
  getStats(): Promise<VoiceInputStats>;
  getStatus(): Promise<VoiceInputStatus>;
  checkPermissions(): Promise<VoiceInputPermissionStatus>;
  startDictation(): Promise<VoiceInputStatus>;
  stopDictation(): Promise<VoiceInputDictationResult>;
  cancelDictation(): Promise<VoiceInputStatus>;
  toggleDictation(): Promise<VoiceInputStatus>;
  openMainWindow(): Promise<void>;
  requestAccessibilityPermission(): Promise<VoiceInputPermissionStatus>;
  openPermissionSettings(target: VoiceInputPermissionSettingsTarget): Promise<string>;
  onStatus(listener: (event: VoiceInputStatusEvent) => void): Promise<UnlistenFn>;
  onWarmupStatus(listener: (event: VoiceInputWarmupStatusEvent) => void): Promise<UnlistenFn>;
  onStatsUpdated(listener: (stats: VoiceInputStats) => void): Promise<UnlistenFn>;
}

// Tauri实现 - 当前就是直接调用Tauri API，保持现有功能不变
class TauriAudioService implements AudioService {
  async requestMicrophonePermission(): Promise<void> {
    return invoke('request_microphone_permission');
  }

  async startRecording(): Promise<void> {
    return invoke('start_recording');
  }

  async stopRecording(args: { save_path: string }): Promise<void> {
    return invoke('stop_recording', { args });
  }

  async playAudioFile(filePath: string): Promise<void> {
    return invoke('play_audio_file', { filePath });
  }

  getLocalAudioUrl(filePath: string): string {
    // 使用 Tauri 的 convertFileSrc 将本地文件路径转换为浏览器可访问的 asset:// URL
    return convertFileSrc(filePath);
  }

  async exportAudioFile(sourcePath: string, targetPath: string): Promise<void> {
    return invoke('export_audio_file', { sourcePath, targetPath });
  }

  async deleteAudioFile(filePath: string): Promise<void> {
    return invoke('delete_audio_file', { filePath });
  }

  async deleteWorkspaceRecording(workspaceFolder: string, recordingId: string): Promise<void> {
    return invoke('delete_workspace_recording', { workspaceFolder, recordingId });
  }
}

class TauriFileService implements FileService {
  async saveTextFile(content: string, targetPath: string): Promise<void> {
    return invoke('save_text_file', { content, targetPath });
  }
}

class TauriVoiceInputService implements VoiceInputService {
  async getStats(): Promise<VoiceInputStats> {
    return invoke('get_voice_input_stats');
  }

  async getStatus(): Promise<VoiceInputStatus> {
    return invoke('get_voice_input_status');
  }

  async checkPermissions(): Promise<VoiceInputPermissionStatus> {
    return invoke('check_voice_input_permissions');
  }

  async startDictation(): Promise<VoiceInputStatus> {
    return invoke('start_voice_input_dictation');
  }

  async stopDictation(): Promise<VoiceInputDictationResult> {
    return invoke('stop_voice_input_dictation');
  }

  async cancelDictation(): Promise<VoiceInputStatus> {
    return invoke('cancel_voice_input_dictation');
  }

  async toggleDictation(): Promise<VoiceInputStatus> {
    return invoke('toggle_voice_input_dictation');
  }

  async openMainWindow(): Promise<void> {
    return invoke('open_main_window_from_voice_input_overlay');
  }

  async requestAccessibilityPermission(): Promise<VoiceInputPermissionStatus> {
    return invoke('request_voice_input_accessibility_permission');
  }

  async openPermissionSettings(target: VoiceInputPermissionSettingsTarget): Promise<string> {
    return invoke('open_audio_permission_settings', { target });
  }

  async onStatus(listener: (event: VoiceInputStatusEvent) => void): Promise<UnlistenFn> {
    return listen<VoiceInputStatusEvent>('voice-input-status', event => listener(event.payload));
  }

  async onWarmupStatus(listener: (event: VoiceInputWarmupStatusEvent) => void): Promise<UnlistenFn> {
    return listen<VoiceInputWarmupStatusEvent>('voice-input-warmup-status', event => listener(event.payload));
  }

  async onStatsUpdated(listener: (stats: VoiceInputStats) => void): Promise<UnlistenFn> {
    return listen<VoiceInputStats>('voice-input-stats-updated', event => listener(event.payload));
  }
}

// 服务实例 - 目前只有Tauri实现
export const audioService: AudioService = new TauriAudioService();
export const fileService: FileService = new TauriFileService();
export const voiceInputService: VoiceInputService = new TauriVoiceInputService();
