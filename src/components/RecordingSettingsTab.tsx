import { useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  AlertCircle,
  Bot,
  Brain,
  CheckCircle2,
  Download,
  ExternalLink,
  Keyboard,
  List,
  Loader2,
  Mic2,
  Plus,
  RefreshCw,
  Save,
  Settings,
  Square,
  Trash2,
  Wand2,
} from 'lucide-react';
import {
  AppSettings,
  AssistantPromptTemplate,
  ModelDownloadProgress,
  ModelStatus,
  RuntimeDependencyStatus,
  SummaryTemplate,
  VoiceInputPermissionStatus,
} from '../appTypes';
import { AppFontSize } from '../hooks/useAppearance';
import { useVoiceInputService } from '../hooks/useServices';
import type { VoiceInputPermissionSettingsTarget } from '../services';
import './RecordingSettingsTab.css';

export type RecordingSettingsPane = 'general' | 'templates' | 'assistant' | 'voiceInput' | 'diagnostics';

interface AudioPermissionStatus {
  platform: string;
  microphone_ok: boolean;
  microphone_message: string;
  system_audio_ok: boolean;
  system_audio_message: string;
  opened_settings: boolean;
  settings_message: string;
}

const emptyTemplate: SummaryTemplate = {
  id: '',
  name: '',
  description: '',
  prompt: '请根据下面的转写文本整理内容。\n\n转写文本：\n{{ transcript }}',
  is_builtin: false,
  created_at: '',
  updated_at: '',
};

const emptyAssistantTemplate: AssistantPromptTemplate = {
  id: '',
  name: '',
  description: '',
  prompt: [
    '你是本地笔记问答助手。',
    '必须先使用 list_notes 或 grep_notes 在本次请求允许的笔记范围内查找依据，再使用 read_note_file 读取相关笔记片段。',
    '只根据工具读取到的笔记内容回答。',
    '如果工具没有找到依据，明确说明未在当前范围的笔记中找到相关信息。',
  ].join('\n'),
  is_builtin: false,
  created_at: '',
  updated_at: '',
};

interface Props {
  initialPane?: RecordingSettingsPane;
  showPaneTabs?: boolean;
  darkMode?: boolean;
  setDarkMode?: (value: boolean) => void;
  fontSize?: AppFontSize;
  setFontSize?: (value: AppFontSize) => void;
  onError: (error: string) => void;
  onClearError: () => void;
}

export function RecordingSettingsTab({ initialPane, showPaneTabs = true, darkMode, setDarkMode, fontSize, setFontSize, onError, onClearError }: Props) {
  const voiceInputService = useVoiceInputService();
  const [activePane, setActivePane] = useState<RecordingSettingsPane>(initialPane || 'general');
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [settingsDraft, setSettingsDraft] = useState<AppSettings | null>(null);
  const [apiKeyDraft, setApiKeyDraft] = useState('');
  const [templates, setTemplates] = useState<SummaryTemplate[]>([]);
  const [templateDraft, setTemplateDraft] = useState<SummaryTemplate>(emptyTemplate);
  const [assistantTemplates, setAssistantTemplates] = useState<AssistantPromptTemplate[]>([]);
  const [assistantTemplateDraft, setAssistantTemplateDraft] = useState<AssistantPromptTemplate>(emptyAssistantTemplate);
  const [modelStatus, setModelStatus] = useState<ModelStatus | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<ModelDownloadProgress | null>(null);
  const [dependencyStatus, setDependencyStatus] = useState<RuntimeDependencyStatus | null>(null);
  const [audioPermissionStatus, setAudioPermissionStatus] = useState<AudioPermissionStatus | null>(null);
  const [voiceInputPermissionStatus, setVoiceInputPermissionStatus] = useState<VoiceInputPermissionStatus | null>(null);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const [localError, setLocalError] = useState<string | null>(null);
  const progressSampleRef = useRef({
    at: Date.now(),
    bytes: 0,
    speed: 0,
  });

  const selectedTemplate = useMemo(
    () => templates.find(template => template.id === templateDraft.id) || null,
    [templates, templateDraft.id]
  );
  const selectedAssistantTemplate = useMemo(
    () => assistantTemplates.find(template => template.id === assistantTemplateDraft.id) || null,
    [assistantTemplates, assistantTemplateDraft.id]
  );

  useEffect(() => {
    void initialize();
  }, []);

  useEffect(() => {
    if (initialPane) setActivePane(initialPane);
  }, [initialPane]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void listen<ModelDownloadProgress>('asr-model-download-progress', event => {
      if (!disposed) {
        progressSampleRef.current = {
          at: Date.now(),
          bytes: event.payload.downloaded_bytes,
          speed: event.payload.speed_bytes_per_second,
        };
        setDownloadProgress(event.payload);
      }
    }).then(dispose => {
      if (disposed) {
        dispose();
      } else {
        unlisten = dispose;
      }
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (busyAction !== '准备本地模型') return;

    let cancelled = false;
    const timer = window.setInterval(() => {
      void invoke<ModelDownloadProgress>('get_model_download_progress')
        .then(snapshot => {
          if (cancelled) return;
          const now = Date.now();
          const previous = progressSampleRef.current;
          const elapsed = Math.max(0.001, (now - previous.at) / 1000);
          const calculatedSpeed = Math.max(0, (snapshot.downloaded_bytes - previous.bytes) / elapsed);
          const speed = snapshot.speed_bytes_per_second > 0
            ? snapshot.speed_bytes_per_second
            : calculatedSpeed > 0
              ? calculatedSpeed
              : previous.speed;
          progressSampleRef.current = {
            at: now,
            bytes: snapshot.downloaded_bytes,
            speed,
          };
          setDownloadProgress(current => ({
            ...snapshot,
            speed_bytes_per_second: speed > 0 ? speed : current?.speed_bytes_per_second ?? 0,
          }));
        })
        .catch(caught => {
          if (!cancelled) {
            console.warn('Failed to poll model download progress', caught);
          }
        });
    }, 1000);

    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [busyAction]);

  async function initialize() {
    await Promise.all([
      loadSettings(),
      loadTemplates(),
      loadAssistantTemplates(),
      refreshDependencies(),
      refreshModelStatus(),
      refreshVoiceInputPermissions(),
    ]);
  }

  async function loadSettings() {
    const nextSettings = await invoke<AppSettings>('get_settings');
    setSettings(nextSettings);
    setSettingsDraft(nextSettings);
    return nextSettings;
  }

  async function loadTemplates() {
    const list = await invoke<SummaryTemplate[]>('list_summary_templates');
    setTemplates(list);
    const defaultTemplate = list.find(template => template.id === 'builtin-meeting-minutes') || list[0];
    setTemplateDraft(current => (current.id ? current : defaultTemplate || emptyTemplate));
    return list;
  }

  async function loadAssistantTemplates() {
    const list = await invoke<AssistantPromptTemplate[]>('list_assistant_prompt_templates');
    setAssistantTemplates(list);
    const defaultTemplate = list.find(template => template.id === 'builtin-local-notes-qa') || list[0];
    setAssistantTemplateDraft(current => (current.id ? current : defaultTemplate || emptyAssistantTemplate));
    return list;
  }

  async function refreshModelStatus() {
    const status = await invoke<ModelStatus>('get_model_status');
    setModelStatus(status);
    return status;
  }

  async function refreshDependencies() {
    const status = await invoke<RuntimeDependencyStatus>('check_runtime_dependencies');
    setDependencyStatus(status);
    return status;
  }

  async function refreshVoiceInputPermissions() {
    const status = await voiceInputService.checkPermissions();
    setVoiceInputPermissionStatus(status);
    return status;
  }

  async function checkDependencies() {
    await runAction('检测运行环境', async () => {
      await refreshDependencies();
    });
  }

  async function checkVoiceInputPermissions() {
    await runAction('检查语音输入法权限', async () => {
      const status = await refreshVoiceInputPermissions();
      if (status.microphone_ok && status.accessibility_ok) {
        setStatusMessage('语音输入法权限正常');
      } else {
        setStatusMessage('语音输入法仍有权限需要确认');
      }
    });
  }

  async function requestVoiceInputAccessibilityPermission() {
    const status = await runAction('请求辅助功能授权', async () => {
      const status = await voiceInputService.requestAccessibilityPermission();
      setVoiceInputPermissionStatus(status);
      return status;
    });
    if (!status) return;
    setStatusMessage(status.accessibility_ok ? '辅助功能权限已授权' : status.accessibility_message);
  }

  async function verifyAudioPermissions() {
    await runAction('验证录音权限', async () => {
      const status = await invoke<AudioPermissionStatus>('verify_audio_permissions', {
        openSettingsOnFailure: true,
      });
      setAudioPermissionStatus(status);
      const platformLabel = platformName(status.platform);
      if (status.microphone_ok && status.system_audio_ok) {
        setStatusMessage(`${platformLabel} 录音权限正常`);
      } else {
        setStatusMessage(`${platformLabel} 权限需确认：${status.settings_message}`);
      }
    });
  }

  async function openAudioPermissionSettings(target: 'microphone' | 'system_audio') {
    await runAction('打开权限设置', async () => {
      const message = await invoke<string>('open_audio_permission_settings', { target });
      setStatusMessage(message);
    });
  }

  async function openVoiceInputPermissionSettings(target: VoiceInputPermissionSettingsTarget) {
    await runAction('打开语音输入法权限设置', async () => {
      const message = await voiceInputService.openPermissionSettings(target);
      setStatusMessage(message);
    });
  }

  async function runAction<T>(label: string, action: () => Promise<T>): Promise<T | null> {
    setBusyAction(label);
    setLocalError(null);
    setStatusMessage(null);
    onClearError();

    try {
      const result = await action();
      setStatusMessage(`${label}完成`);
      return result;
    } catch (caught) {
      const message = errorText(caught);
      setLocalError(message);
      onError(message);
      return null;
    } finally {
      setBusyAction(null);
    }
  }

  async function saveSettingsDraft() {
    if (!settingsDraft) return;
    await runAction('保存录音配置', async () => {
      const nextSettings = await invoke<AppSettings>('save_settings', { settings: settingsDraft });
      setSettings(nextSettings);
      setSettingsDraft(nextSettings);
      if (apiKeyDraft.trim()) {
        await invoke('set_llm_api_key', { apiKey: apiKeyDraft });
        setApiKeyDraft('');
        await loadSettings();
      }
      await Promise.all([refreshDependencies(), refreshModelStatus()]);
    });
  }

  async function testLlmProvider() {
    await runAction('测试 LLM', async () => {
      if (apiKeyDraft.trim()) {
        await invoke('set_llm_api_key', { apiKey: apiKeyDraft });
        setApiKeyDraft('');
        await loadSettings();
      }
      const message = await invoke<string>('test_llm_provider');
      setStatusMessage(`LLM 连接正常：${message}`);
    });
  }

  async function ensureModel() {
    await runAction('准备本地模型', async () => {
      if (settingsDraft) {
        const nextSettings = await invoke<AppSettings>('save_settings', { settings: settingsDraft });
        setSettings(nextSettings);
        setSettingsDraft(nextSettings);
      }
      const status = await invoke<ModelStatus>('ensure_asr_model');
      setModelStatus(status);
      setDownloadProgress(current => current ? { ...current, status: 'ready', percent: 100, message: 'FunASR workflow models are ready' } : current);
    });
  }

  async function cancelModelDownload() {
    try {
      const status = await invoke<ModelStatus>('cancel_asr_model_download');
      setModelStatus(status);
      const snapshot = await invoke<ModelDownloadProgress>('get_model_download_progress');
      setDownloadProgress(snapshot);
      setStatusMessage('模型下载已中断');
    } catch (caught) {
      const message = errorText(caught);
      setLocalError(message);
      onError(message);
    }
  }

  async function saveTemplateDraft() {
    await runAction('保存模板', async () => {
      const saved = await invoke<SummaryTemplate>('save_summary_template', {
        id: templateDraft.id || null,
        name: templateDraft.name,
        description: templateDraft.description || null,
        prompt: templateDraft.prompt,
      });
      setTemplateDraft(saved);
      await loadTemplates();
    });
  }

  async function deleteTemplateDraft() {
    if (!templateDraft.id || templateDraft.is_builtin) return;
    await runAction('删除模板', async () => {
      await invoke('delete_summary_template', { id: templateDraft.id });
      const list = await loadTemplates();
      setTemplateDraft(list[0] || emptyTemplate);
    });
  }

  async function saveAssistantTemplateDraft() {
    await runAction('保存 AI 问答模板', async () => {
      const saved = await invoke<AssistantPromptTemplate>('save_assistant_prompt_template', {
        id: assistantTemplateDraft.id || null,
        name: assistantTemplateDraft.name,
        description: assistantTemplateDraft.description || null,
        prompt: assistantTemplateDraft.prompt,
      });
      setAssistantTemplateDraft(saved);
      await loadAssistantTemplates();
    });
  }

  async function deleteAssistantTemplateDraft() {
    if (!assistantTemplateDraft.id || assistantTemplateDraft.is_builtin) return;
    await runAction('删除 AI 问答模板', async () => {
      await invoke('delete_assistant_prompt_template', { id: assistantTemplateDraft.id });
      const list = await loadAssistantTemplates();
      setAssistantTemplateDraft(list[0] || emptyAssistantTemplate);
    });
  }

  function updateSetting<K extends keyof AppSettings>(key: K, value: AppSettings[K]) {
    setSettingsDraft(current => (current ? { ...current, [key]: value } : current));
  }

  if (!settingsDraft) {
    return (
      <div className="recording-settings-tab centered">
        <Loader2 size={18} className="spinning" />
        <span>加载录音配置...</span>
      </div>
    );
  }

  return (
    <div className="recording-settings-tab">
      <div className="recording-settings-inner">
        <div className="recording-settings-title-row">
          <div>
            <h2 className="recording-settings-title">{paneTitle(activePane)}</h2>
            <p className="recording-settings-subtitle">{paneSubtitle(activePane)}</p>
          </div>
          {busyAction && (
            <span className="config-status-pill warn">
              <Loader2 size={13} className="spinning" /> {busyAction}
            </span>
          )}
        </div>

        {(localError || statusMessage) && (
          <div className={`config-notice ${localError ? 'error' : 'success'}`}>
            {localError ? <AlertCircle size={15} /> : <CheckCircle2 size={15} />}
            <span>{localError || statusMessage}</span>
          </div>
        )}

        {showPaneTabs && (
          <div className="recording-settings-tabs">
            <button
              className={`tab-btn ${activePane === 'general' ? 'active' : ''}`}
              onClick={() => setActivePane('general')}
            >
              <Settings size={14} /> 基础配置
            </button>
            <button
              className={`tab-btn ${activePane === 'templates' ? 'active' : ''}`}
              onClick={() => setActivePane('templates')}
            >
              <List size={14} /> 模板管理
            </button>
            <button
              className={`tab-btn ${activePane === 'assistant' ? 'active' : ''}`}
              onClick={() => setActivePane('assistant')}
            >
              <Bot size={14} /> AI问答
            </button>
            <button
              className={`tab-btn ${activePane === 'voiceInput' ? 'active' : ''}`}
              onClick={() => setActivePane('voiceInput')}
            >
              <Mic2 size={14} /> 语音输入法
            </button>
            <button
              className={`tab-btn ${activePane === 'diagnostics' ? 'active' : ''}`}
              onClick={() => setActivePane('diagnostics')}
            >
              <AlertCircle size={14} /> 检测
            </button>
          </div>
        )}

        {activePane === 'general' && (
          <div className="settings-panel">
            <div className="settings-top-actions">
              <button className="btn-primary" onClick={() => void saveSettingsDraft()} disabled={!!busyAction}>
                <Save size={14} /> 保存配置
              </button>
              <button className="btn-secondary" onClick={() => void testLlmProvider()} disabled={!!busyAction}>
                <Brain size={14} /> 测试 LLM
              </button>
            </div>
            {(setDarkMode || setFontSize) && (
              <div className="asr-engine-summary">
                <div>
                  <div className="asr-engine-title">界面</div>
                  <div className="asr-engine-copy">调整全局界面字号和深色显示。</div>
                </div>
                <div className="appearance-controls">
                  {setFontSize && fontSize && (
                    <div className="font-size-control" aria-label="字体大小">
                      <button
                        className={fontSize === 'small' ? 'active' : ''}
                        type="button"
                        onClick={() => setFontSize('small')}
                      >
                        小
                      </button>
                      <button
                        className={fontSize === 'standard' ? 'active' : ''}
                        type="button"
                        onClick={() => setFontSize('standard')}
                      >
                        标准
                      </button>
                      <button
                        className={fontSize === 'large' ? 'active' : ''}
                        type="button"
                        onClick={() => setFontSize('large')}
                      >
                        大
                      </button>
                    </div>
                  )}
                  {setDarkMode && (
                    <label className="settings-toggle">
                      <input
                        type="checkbox"
                        checked={!!darkMode}
                        onChange={event => setDarkMode(event.target.checked)}
                      />
                      <span>夜间模式</span>
                    </label>
                  )}
                </div>
              </div>
            )}
            <div className="asr-engine-summary">
              <div>
                <div className="asr-engine-title">FunASR / paraformer-zh + CAM++ + CT Punc</div>
                <div className="asr-engine-copy">
                  默认使用本地 FunASR 流水线，内置 VAD、说话人识别和标点恢复。
                </div>
              </div>
              <span className="config-status-pill ok">本机部署</span>
            </div>
            <div className="settings-grid">
              <TextField label="FunASR 模型仓库" value={settingsDraft.asr_model_repo} onChange={value => updateSetting('asr_model_repo', value)} />
              <SelectField
                label="模型下载源"
                value={settingsDraft.asr_model_source || 'modelscope'}
                onChange={value => updateSetting('asr_model_source', value)}
                options={[
                  { value: 'modelscope', label: 'ModelScope 魔塔' },
                  { value: 'huggingface', label: 'Hugging Face' },
                ]}
              />
              <TextField label="本地 ASR 模型目录（可选）" value={settingsDraft.asr_model_path || ''} onChange={value => updateSetting('asr_model_path', value || null)} />
              <TextField label="HTTP Proxy（可选）" value={settingsDraft.http_proxy || ''} onChange={value => updateSetting('http_proxy', value || null)} />
              <TextField label="HTTPS Proxy（可选）" value={settingsDraft.https_proxy || ''} onChange={value => updateSetting('https_proxy', value || null)} />
              <TextField label="SOCKS / ALL Proxy（可选）" value={settingsDraft.all_proxy || ''} onChange={value => updateSetting('all_proxy', value || null)} />
              <TextField label="LLM Provider" value={settingsDraft.llm_provider} onChange={value => updateSetting('llm_provider', value)} />
              <TextField label="LLM Base URL" value={settingsDraft.llm_base_url} onChange={value => updateSetting('llm_base_url', value)} />
              <TextField label="LLM Model" value={settingsDraft.llm_model} onChange={value => updateSetting('llm_model', value)} />
              <TextField label="API Key（留空不修改）" type="password" value={apiKeyDraft} onChange={setApiKeyDraft} />
              <NumberField label="Temperature" value={settingsDraft.llm_temperature} onChange={value => updateSetting('llm_temperature', value)} />
              <NumberField label="Max Tokens" value={settingsDraft.llm_max_tokens} onChange={value => updateSetting('llm_max_tokens', value)} />
              <NumberField label="Timeout 秒" value={settingsDraft.llm_timeout_seconds} onChange={value => updateSetting('llm_timeout_seconds', value)} />
            </div>

            <div className="recording-settings-footer">
              <span>API Key 保存在系统 keychain；模型和普通设置保存在本机目录。</span>
            </div>
          </div>
        )}

        {activePane === 'templates' && (
          <div className="settings-panel">
            <div className="settings-top-actions">
              <button className="btn-secondary" onClick={() => setTemplateDraft(emptyTemplate)}>
                <Plus size={14} /> 新建模板
              </button>
              <button className="btn-primary" onClick={() => void saveTemplateDraft()} disabled={templateDraft.is_builtin || !!busyAction}>
                <Save size={14} /> 保存模板
              </button>
              <button className="btn-danger" onClick={() => void deleteTemplateDraft()} disabled={!templateDraft.id || templateDraft.is_builtin || !!busyAction}>
                <Trash2 size={14} /> 删除模板
              </button>
            </div>
            <div className="template-settings-grid">
              <div className="template-list-panel">
                <div className="template-list">
                  {templates.map(template => (
                    <button
                      key={template.id}
                      className={`template-item ${templateDraft.id === template.id ? 'active' : ''}`}
                      onClick={() => setTemplateDraft(template)}
                    >
                      <span className="template-name">{template.name}</span>
                      <span className="template-desc">{template.description || (template.is_builtin ? '内置模板' : '自定义模板')}</span>
                    </button>
                  ))}
                </div>
              </div>

              <div className="template-editor-panel">
                <TextField label="模板名称" value={templateDraft.name} onChange={value => setTemplateDraft(current => ({ ...current, name: value }))} disabled={templateDraft.is_builtin} />
                <TextField label="描述" value={templateDraft.description || ''} onChange={value => setTemplateDraft(current => ({ ...current, description: value }))} disabled={templateDraft.is_builtin} />
                <div className="settings-field">
                  <label>Prompt</label>
                  <textarea
                    value={templateDraft.prompt}
                    onChange={event => setTemplateDraft(current => ({ ...current, prompt: event.target.value }))}
                    disabled={templateDraft.is_builtin}
                    className="template-prompt"
                  />
                </div>
                {selectedTemplate?.is_builtin && (
                  <p className="settings-help">内置模板不可编辑。可以点击“新建模板”创建自定义版本。</p>
                )}
              </div>
            </div>
          </div>
        )}

        {activePane === 'assistant' && (
          <div className="settings-panel">
            <div className="settings-top-actions">
              <button className="btn-secondary" onClick={() => setAssistantTemplateDraft(emptyAssistantTemplate)}>
                <Plus size={14} /> 新建模板
              </button>
              <button className="btn-primary" onClick={() => void saveAssistantTemplateDraft()} disabled={assistantTemplateDraft.is_builtin || !!busyAction}>
                <Save size={14} /> 保存模板
              </button>
              <button className="btn-danger" onClick={() => void deleteAssistantTemplateDraft()} disabled={!assistantTemplateDraft.id || assistantTemplateDraft.is_builtin || !!busyAction}>
                <Trash2 size={14} /> 删除模板
              </button>
            </div>
            <div className="template-settings-grid">
              <div className="template-list-panel">
                <div className="template-list">
                  {assistantTemplates.map(template => (
                    <button
                      key={template.id}
                      className={`template-item ${assistantTemplateDraft.id === template.id ? 'active' : ''}`}
                      onClick={() => setAssistantTemplateDraft(template)}
                    >
                      <span className="template-name">{template.name}</span>
                      <span className="template-desc">{template.description || (template.is_builtin ? '内置模板' : '自定义模板')}</span>
                    </button>
                  ))}
                </div>
              </div>

              <div className="template-editor-panel">
                <TextField label="模板名称" value={assistantTemplateDraft.name} onChange={value => setAssistantTemplateDraft(current => ({ ...current, name: value }))} disabled={assistantTemplateDraft.is_builtin} />
                <TextField label="描述" value={assistantTemplateDraft.description || ''} onChange={value => setAssistantTemplateDraft(current => ({ ...current, description: value }))} disabled={assistantTemplateDraft.is_builtin} />
                <div className="settings-field">
                  <label>Agent Prompt</label>
                  <textarea
                    value={assistantTemplateDraft.prompt}
                    onChange={event => setAssistantTemplateDraft(current => ({ ...current, prompt: event.target.value }))}
                    disabled={assistantTemplateDraft.is_builtin}
                    className="template-prompt"
                  />
                </div>
                {selectedAssistantTemplate?.is_builtin && (
                  <p className="settings-help">内置 AI 问答模板不可编辑。可以点击“新建模板”创建自定义版本。</p>
                )}
                <p className="settings-help">问答时可在右侧 AI 问答面板切换模板；每次提问都会重新按当前范围读取笔记。</p>
              </div>
            </div>
          </div>
        )}

        {activePane === 'voiceInput' && (
          <div className="settings-panel">
            <div className="settings-top-actions">
              <button className="btn-primary" onClick={() => void saveSettingsDraft()} disabled={!!busyAction}>
                <Save size={14} /> 保存配置
              </button>
              <button className="btn-secondary" onClick={() => void checkVoiceInputPermissions()} disabled={!!busyAction}>
                <Keyboard size={14} /> 检查权限
              </button>
              {voiceInputPermissionStatus && !voiceInputPermissionStatus.microphone_ok && (
                <button className="btn-secondary" onClick={() => void openVoiceInputPermissionSettings('microphone')} disabled={!!busyAction}>
                  <ExternalLink size={14} /> 打开麦克风权限设置
                </button>
              )}
              {voiceInputPermissionStatus && !voiceInputPermissionStatus.accessibility_ok && (
                <>
                  <button className="btn-secondary" onClick={() => void requestVoiceInputAccessibilityPermission()} disabled={!!busyAction}>
                    <ExternalLink size={14} /> 请求辅助功能授权
                  </button>
                  <button className="btn-secondary" onClick={() => void openVoiceInputPermissionSettings('accessibility')} disabled={!!busyAction}>
                    <ExternalLink size={14} /> 打开辅助功能权限设置
                  </button>
                </>
              )}
            </div>
            <div className="asr-engine-summary">
              <div>
                <div className="asr-engine-title">全局语音输入</div>
                <div className="asr-engine-copy">
                  聚焦任意普通可编辑输入框后，用全局快捷键触发短语音听写；默认本地 ASR 直出，AI 润色需手动开启。
                </div>
              </div>
              <span className={`config-status-pill ${settingsDraft.voice_input_enabled ? 'ok' : 'warn'}`}>
                {settingsDraft.voice_input_enabled ? '已启用' : '未启用'}
              </span>
            </div>

            <div className="settings-grid">
              <label className="voice-input-switch-card">
                <input
                  type="checkbox"
                  checked={settingsDraft.voice_input_enabled}
                  onChange={event => updateSetting('voice_input_enabled', event.target.checked)}
                />
                <span>
                  <strong>启用语音输入法</strong>
                  <small>启用后后台监听全局快捷键；会议录音进行中会拒绝启动。</small>
                </span>
              </label>
              <HotkeyCaptureField
                label="全局快捷键"
                value={settingsDraft.voice_input_hotkey}
                onChange={value => updateSetting('voice_input_hotkey', value)}
              />
              <SelectField
                label="输出模式"
                value={settingsDraft.voice_input_refinement_mode || 'local'}
                onChange={value => updateSetting('voice_input_refinement_mode', value)}
                options={[
                  { value: 'local', label: '本地直出' },
                  { value: 'ai_polish', label: 'AI 润色' },
                ]}
              />
              <div className="voice-input-mode-note">
                <Wand2 size={15} />
                <span>AI 润色复用上方 LLM Provider、Base URL、Model 和 API Key；基础语音输入不依赖云端。</span>
              </div>
            </div>

            <div className="settings-field">
              <label>语音修正 Prompt</label>
              <textarea
                value={settingsDraft.voice_input_refinement_prompt}
                onChange={event => updateSetting('voice_input_refinement_prompt', event.target.value)}
                className="voice-input-prompt"
              />
              <p className="settings-help">
                支持 <code>{'{{ transcript }}'}</code> 或 <code>{'{{ text }}'}</code> 占位符；留空保存会恢复默认 prompt。
              </p>
            </div>

            <div className="diagnostics-grid">
              <Info
                label="麦克风"
                value={voiceInputPermissionStatus?.microphone_message || permissionPlaceholder()}
                tone={voiceInputPermissionStatus ? permissionTone(voiceInputPermissionStatus.microphone_ok) : 'neutral'}
              />
              <Info
                label="Accessibility"
                value={voiceInputPermissionStatus?.accessibility_message || '未验证\n自动写入当前输入框需要 macOS Accessibility 权限'}
                tone={voiceInputPermissionStatus ? permissionTone(voiceInputPermissionStatus.accessibility_ok) : 'neutral'}
              />
              <Info
                label="插入策略"
                value="优先 Accessibility 直接替换当前焦点选区；不可用时尝试剪贴板粘贴，失败时保留文本到剪贴板。"
              />
              <Info
                label="系统限制"
                value="密码框、安全输入、阻止粘贴的应用、远程桌面、虚拟机和游戏窗口不承诺绕过系统限制。"
              />
            </div>

            <div className="recording-settings-footer">
              <span>快捷键会在保存后重新注册；若被系统或其他应用占用，保存时会提示失败。</span>
            </div>
          </div>
        )}

        {activePane === 'diagnostics' && (
          <div className="settings-panel">
            <div className="settings-top-actions">
              <button className="btn-secondary" onClick={() => void verifyAudioPermissions()} disabled={!!busyAction}>
                <ExternalLink size={14} /> 验证录音权限
              </button>
              {audioPermissionStatus && !audioPermissionStatus.microphone_ok && (
                <button className="btn-secondary" onClick={() => void openAudioPermissionSettings('microphone')} disabled={!!busyAction}>
                  <ExternalLink size={14} /> 打开麦克风权限设置
                </button>
              )}
              {audioPermissionStatus && !audioPermissionStatus.system_audio_ok && (
                <button className="btn-secondary" onClick={() => void openAudioPermissionSettings('system_audio')} disabled={!!busyAction}>
                  <ExternalLink size={14} /> 打开系统音频权限设置
                </button>
              )}
              <button className="btn-secondary" onClick={() => void checkDependencies()} disabled={!!busyAction}>
                <RefreshCw size={14} /> 检测运行环境
              </button>
              <button className="btn-secondary" onClick={() => void ensureModel()} disabled={!!busyAction}>
                <Download size={14} /> 下载/检查 FunASR workflow 模型
              </button>
              <button
                className="btn-danger"
                onClick={() => void cancelModelDownload()}
                disabled={busyAction !== '准备本地模型' && downloadProgress?.status !== 'downloading'}
              >
                <Square size={14} /> 中断下载
              </button>
              <button className="btn-secondary" onClick={() => void testLlmProvider()} disabled={!!busyAction}>
                <Brain size={14} /> 测试 LLM
              </button>
            </div>
            <div className="diagnostics-grid">
              <Info label="内置 Python" value={`${dependencyStatus?.python_ok ? 'OK' : '异常'}：${dependencyStatus?.python_message || '未检测'}`} />
              <Info label="内置 ffmpeg" value={`${dependencyStatus?.ffmpeg_ok ? 'OK' : '异常'}：${dependencyStatus?.ffmpeg_message || '未检测'}`} />
              <Info label="ASR 引擎" value="FunASR Workflow" tone="ok" />
              <Info label="模型状态" value={modelStatusText(modelStatus, settingsDraft.asr_model_source)} tone={statusTone(modelStatus?.status)} />
              <Info label="LLM Key" value={settings?.has_llm_api_key ? '已配置' : '未配置'} tone={settings?.has_llm_api_key ? 'ok' : 'neutral'} />
              <Info
                label="麦克风权限"
                value={audioPermissionStatus?.microphone_message || permissionPlaceholder()}
                tone={audioPermissionStatus ? permissionTone(audioPermissionStatus.microphone_ok) : 'neutral'}
              />
              <Info
                label={systemAudioLabel(audioPermissionStatus?.platform)}
                value={audioPermissionStatus?.system_audio_message || permissionPlaceholder()}
                tone={audioPermissionStatus ? permissionTone(audioPermissionStatus.system_audio_ok) : 'neutral'}
              />
            </div>
            {downloadProgress && (
              <div className="download-progress-panel">
                <div className="download-progress-header">
                  <span>{downloadProgressTitle(downloadProgress)}</span>
                  <span>{formatPercent(downloadProgress.percent)}</span>
                </div>
                <div className="download-progress-track">
                  <div
                    className="download-progress-fill"
                    style={{ width: `${Math.max(2, Math.min(100, downloadProgress.percent ?? 0))}%` }}
                  />
                </div>
                <div className="download-progress-meta">
                  <span>{formatBytes(downloadProgress.downloaded_bytes)} / {downloadProgress.total_bytes ? formatBytes(downloadProgress.total_bytes) : '未知大小'}</span>
                  <span>{formatBytes(downloadProgress.speed_bytes_per_second)}/s</span>
                </div>
                {downloadProgress.message && <div className="download-progress-message">{downloadProgress.message}</div>}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function TextField({
  label,
  value,
  onChange,
  type = 'text',
  disabled = false,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  disabled?: boolean;
}) {
  return (
    <div className="settings-field">
      <label>{label}</label>
      <input type={type} value={value} onChange={event => onChange(event.target.value)} disabled={disabled} />
    </div>
  );
}

function HotkeyCaptureField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  const [capturing, setCapturing] = useState(false);
  const [captureError, setCaptureError] = useState<string | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const modifierCaptureTimeoutRef = useRef<number | null>(null);

  useEffect(() => {
    if (!capturing) return;

    const clearModifierCaptureTimeout = () => {
      if (modifierCaptureTimeoutRef.current) {
        window.clearTimeout(modifierCaptureTimeoutRef.current);
        modifierCaptureTimeoutRef.current = null;
      }
    };

    const commitHotkey = (nextHotkey: string) => {
      clearModifierCaptureTimeout();
      onChange(nextHotkey);
      setCapturing(false);
      setCaptureError(null);
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();

      if (event.key === 'Escape' && !hasAnyModifier(event)) {
        setCapturing(false);
        setCaptureError(null);
        return;
      }

      const nextHotkey = hotkeyFromKeyboardEvent(event);
      if (!nextHotkey) {
        setCaptureError('请按下 Command、Option、Control，或修饰键加普通键');
        return;
      }

      if (isModifierOnlyHotkey(nextHotkey)) {
        clearModifierCaptureTimeout();
        modifierCaptureTimeoutRef.current = window.setTimeout(() => {
          commitHotkey(nextHotkey);
        }, 320);
        setCaptureError(null);
        return;
      }

      commitHotkey(nextHotkey);
    };

    window.addEventListener('keydown', handleKeyDown, true);
    return () => {
      window.removeEventListener('keydown', handleKeyDown, true);
      clearModifierCaptureTimeout();
    };
  }, [capturing, onChange]);

  const startCapture = () => {
    setCapturing(true);
    setCaptureError(null);
    window.setTimeout(() => buttonRef.current?.focus(), 0);
  };

  return (
    <div className="settings-field">
      <label>{label}</label>
      <button
        ref={buttonRef}
        type="button"
        className={`hotkey-capture-button ${capturing ? 'capturing' : ''}`}
        onClick={startCapture}
      >
        <Keyboard size={14} />
        <span>{capturing ? '按下新的快捷键' : displayHotkey(value)}</span>
      </button>
      {captureError && <p className="settings-help hotkey-capture-error">{captureError}</p>}
    </div>
  );
}

function hasAnyModifier(event: KeyboardEvent) {
  return event.metaKey || event.ctrlKey || event.altKey || event.shiftKey;
}

function hotkeyFromKeyboardEvent(event: KeyboardEvent): string | null {
  const modifierOnlyHotkey = modifierOnlyHotkeyFromKeyboardEvent(event);
  if (modifierOnlyHotkey) return modifierOnlyHotkey;

  const key = hotkeyKeyToken(event);
  if (!key) return null;

  if (!hasAnyModifier(event)) return null;

  const modifiers = modifierTokensFromKeyboardEvent(event);
  return [...modifiers, key].join('+');
}

function modifierOnlyHotkeyFromKeyboardEvent(event: KeyboardEvent): string | null {
  const modifierKeys = new Set(['Alt', 'Control', 'Meta', 'Shift']);
  if (!modifierKeys.has(event.key)) return null;

  const modifiers = modifierTokensFromKeyboardEvent(event);
  if (!hasPrimaryModifier(modifiers)) return null;
  return modifiers.join('+');
}

function modifierTokensFromKeyboardEvent(event: KeyboardEvent) {
  const modifiers: string[] = [];
  if (event.metaKey) modifiers.push('CommandOrControl');
  if (event.ctrlKey) modifiers.push('Control');
  if (event.altKey) modifiers.push('Option');
  if (event.shiftKey) modifiers.push('Shift');
  return modifiers;
}

function hasPrimaryModifier(modifiers: string[]) {
  return modifiers.some(modifier => modifier === 'CommandOrControl' || modifier === 'Control' || modifier === 'Option');
}

function isModifierOnlyHotkey(value: string) {
  const tokens = value.split('+').map(token => token.trim()).filter(Boolean);
  return tokens.length > 0 && tokens.every(token => ['CommandOrControl', 'Control', 'Option', 'Shift'].includes(token));
}

function hotkeyKeyToken(event: KeyboardEvent): string | null {
  const modifierKeys = new Set(['Alt', 'Control', 'Meta', 'Shift']);
  if (modifierKeys.has(event.key)) return null;

  if (event.code === 'Space') return 'Space';
  if (event.key === 'Enter' || event.key === 'Return') return 'Enter';
  if (event.key === 'Escape') return 'Escape';
  if (event.key === 'Tab') return 'Tab';
  if (/^[a-z]$/i.test(event.key)) return event.key.toUpperCase();
  if (/^[0-9]$/.test(event.key)) return event.key;

  const codeTokens: Record<string, string> = {
    Equal: '=',
    Minus: '-',
    BracketRight: ']',
    BracketLeft: '[',
    Quote: "'",
    Semicolon: ';',
    Backslash: '\\',
    Comma: ',',
    Slash: '/',
    Period: '.',
  };
  if (codeTokens[event.code]) return codeTokens[event.code];

  const punctuation: Record<string, string> = {
    '=': '=',
    '-': '-',
    ']': ']',
    '[': '[',
    "'": "'",
    ';': ';',
    '\\': '\\',
    ',': ',',
    '/': '/',
    '.': '.',
  };
  return punctuation[event.key] || null;
}

function displayHotkey(value: string) {
  const tokens = value.split('+').map(token => token.trim()).filter(Boolean);
  if (tokens.length === 0) return '未设置';
  return tokens.map(displayHotkeyToken).join(' ');
}

function displayHotkeyToken(token: string) {
  const normalized = token.replace(/[\s_-]/g, '').toUpperCase();
  switch (normalized) {
    case 'COMMAND':
    case 'CMD':
    case 'META':
    case 'COMMANDORCONTROL':
    case 'PRIMARY':
      return '⌘';
    case 'CONTROL':
    case 'CTRL':
      return '⌃';
    case 'OPTION':
    case 'OPT':
    case 'ALT':
      return '⌥';
    case 'SHIFT':
      return '⇧';
    case 'SPACE':
      return 'Space';
    case 'RETURN':
    case 'ENTER':
      return '↩';
    case 'ESC':
    case 'ESCAPE':
      return 'Esc';
    default:
      return token.length === 1 ? token.toUpperCase() : token;
  }
}

function NumberField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
}) {
  return (
    <div className="settings-field">
      <label>{label}</label>
      <input type="number" value={value} onChange={event => onChange(Number(event.target.value))} />
    </div>
  );
}

function SelectField({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  options: Array<{ value: string; label: string }>;
}) {
  return (
    <div className="settings-field">
      <label>{label}</label>
      <select value={value} onChange={event => onChange(event.target.value)}>
        {options.map(option => (
          <option key={option.value} value={option.value}>{option.label}</option>
        ))}
      </select>
    </div>
  );
}

function paneTitle(pane: RecordingSettingsPane) {
  switch (pane) {
    case 'templates':
      return '模板管理';
    case 'assistant':
      return 'AI问答';
    case 'voiceInput':
      return '语音输入法';
    case 'diagnostics':
      return '检测';
    default:
      return '基础配置';
  }
}

function paneSubtitle(pane: RecordingSettingsPane) {
  switch (pane) {
    case 'templates':
      return '管理会议总结模板和自定义 prompt。';
    case 'assistant':
      return '管理本地笔记问答 Agent 的提示词模板。';
    case 'voiceInput':
      return '配置全局快捷键、本地直出、AI 润色和语音修正 prompt。';
    case 'diagnostics':
      return '检查本地运行环境、权限、模型和 LLM 连接状态。';
    default:
      return '本机 FunASR workflow、LLM、代理和界面偏好设置。';
  }
}

function Info({
  label,
  value,
  tone = 'neutral',
}: {
  label: string;
  value: string;
  tone?: 'ok' | 'warn' | 'bad' | 'neutral';
}) {
  return (
    <div className={`diagnostics-card ${tone}`}>
      <div className="diagnostics-label">{label}</div>
      <div className="diagnostics-value">{value}</div>
    </div>
  );
}

function statusTone(status?: string | null): 'ok' | 'warn' | 'bad' | 'neutral' {
  if (!status || status === 'not_started' || status === 'not_downloaded') return 'neutral';
  if (['ready', 'succeeded', 'ok'].includes(status)) return 'ok';
  if (['running', 'downloading', 'queued'].includes(status)) return 'warn';
  if (['failed', 'missing'].includes(status)) return 'bad';
  return 'neutral';
}

function permissionTone(ok: boolean): 'ok' | 'bad' {
  return ok ? 'ok' : 'bad';
}

function permissionPlaceholder() {
  return '未验证\n点击“验证录音权限”检查系统授权';
}

function systemAudioLabel(platform?: string) {
  return platform === 'macos' ? '系统音频权限' : '系统音频';
}

function platformName(platform: string) {
  switch (platform) {
    case 'macos':
      return 'macOS';
    case 'windows':
      return 'Windows';
    default:
      return platform || '当前系统';
  }
}

function errorText(value: unknown) {
  if (typeof value === 'string') return value;
  if (value instanceof Error) return value.message;
  return JSON.stringify(value);
}

function downloadProgressTitle(progress: ModelDownloadProgress) {
  const source = modelSourceLabel(progress.source);
  if (progress.status === 'ready') return `本地模型已就绪：${source}`;
  if (progress.status === 'failed') return `本地模型下载失败：${source}`;
  if (progress.status === 'cancelled') return `本地模型下载已中断：${source}`;
  return `正在下载本地模型：${source}`;
}

function modelStatusText(status: ModelStatus | null, source: string) {
  if (!status) return '未知\n尚未检测模型状态';
  const label = modelStatusLabel(status.status);
  if (status.path) return `${label}\n${status.path}`;
  if (status.status === 'downloading') return `${label}\n正在从 ${modelSourceLabel(source)} 下载`;
  if (status.message) return `${label}\n${modelStatusMessage(status.message, source)}`;
  return label;
}

function modelStatusLabel(status?: string | null) {
  switch (status) {
    case 'ready':
      return '已就绪';
    case 'downloading':
      return '下载中';
    case 'failed':
      return '下载失败';
    case 'cancelled':
      return '已中断';
    case 'missing':
      return '文件缺失';
    case 'not_downloaded':
    case 'not_started':
      return '未下载';
    default:
      return '未知';
  }
}

function modelStatusMessage(message: string, source: string) {
  if (message.startsWith('Downloading ASR model from') || message.startsWith('Downloading FunASR workflow models from')) {
    return `正在从 ${modelSourceLabel(source)} 下载`;
  }
  if (message === 'ASR model is ready') return 'ASR 模型已就绪';
  if (message === 'FunASR workflow models are ready') return 'FunASR workflow 已就绪';
  if (message === 'ASR workflow auxiliary models are not downloaded') return 'VAD / Speaker / Punc 尚未下载';
  if (message === 'FunASR workflow model download cancelled') return '模型下载已中断';
  if (message === 'No active FunASR workflow model download') return '当前没有正在进行的模型下载';
  return message;
}

function modelSourceLabel(source?: string | null) {
  return source === 'modelscope' ? 'ModelScope 魔塔' : 'Hugging Face';
}

function formatPercent(value?: number | null) {
  if (value == null || Number.isNaN(value)) return '估算中';
  return `${value.toFixed(value >= 10 ? 1 : 2)}%`;
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let next = value;
  let index = 0;
  while (next >= 1024 && index < units.length - 1) {
    next /= 1024;
    index += 1;
  }
  return `${next.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}
