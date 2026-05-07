import { useState } from 'react';
import { AlertCircle, Bot, Database, List, Mic2, Settings, X } from 'lucide-react';
import { AppFontSize } from '../hooks/useAppearance';
import { VoiceprintTab } from './VoiceprintTab';
import { type RecordingSettingsPane, RecordingSettingsTab } from './RecordingSettingsTab';
import './SettingsModal.css';

interface Props {
  open: boolean;
  onClose: () => void;
  darkMode: boolean;
  setDarkMode: (v: boolean) => void;
  fontSize: AppFontSize;
  setFontSize: (value: AppFontSize) => void;
  onError: (error: string) => void;
  onClearError: () => void;
}

export default function SettingsModal(props: Props) {
  const { open, onClose } = props;
  const [tab, setTab] = useState<RecordingSettingsPane | 'voiceprint'>('general');
  if (!open) return null;
  return (
    <div className="modal-backdrop">
      <div className="modal-sheet settings-modal-sheet">
        <div className="modal-header">
          <div className="modal-tabs">
            <button className={`tab-btn ${tab === 'general' ? 'active' : ''}`} onClick={() => setTab('general')}>
              <Settings size={16} /> 基础配置
            </button>
            <button className={`tab-btn ${tab === 'templates' ? 'active' : ''}`} onClick={() => setTab('templates')}>
              <List size={16} /> 模板管理
            </button>
            <button className={`tab-btn ${tab === 'assistant' ? 'active' : ''}`} onClick={() => setTab('assistant')}>
              <Bot size={16} /> AI问答
            </button>
            <button className={`tab-btn ${tab === 'voiceInput' ? 'active' : ''}`} onClick={() => setTab('voiceInput')}>
              <Mic2 size={16} /> 语音输入法
            </button>
            <button className={`tab-btn ${tab === 'diagnostics' ? 'active' : ''}`} onClick={() => setTab('diagnostics')}>
              <AlertCircle size={16} /> 检测
            </button>
            <button className={`tab-btn ${tab === 'voiceprint' ? 'active' : ''}`} onClick={() => setTab('voiceprint')}>
              <Database size={16} /> 声纹库
            </button>
          </div>
          <button className="icon-btn" onClick={onClose}><X size={18} /></button>
        </div>
        <div className="modal-body">
          {tab !== 'voiceprint' ? (
            <RecordingSettingsTab
              initialPane={tab}
              showPaneTabs={false}
              darkMode={props.darkMode}
              setDarkMode={props.setDarkMode}
              fontSize={props.fontSize}
              setFontSize={props.setFontSize}
              onError={props.onError}
              onClearError={props.onClearError}
            />
          ) : (
            <VoiceprintTab />
          )}
        </div>
      </div>
    </div>
  );
}
