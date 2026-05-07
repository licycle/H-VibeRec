import { useState } from 'react';
import { Save } from 'lucide-react';
import './SettingsTab.css';

interface SettingsTabProps {
  darkMode: boolean;
  setDarkMode: (v: boolean) => void;
  onError: (error: string) => void;
}

export function SettingsTab({
  darkMode,
  setDarkMode,
  onError,
}: SettingsTabProps) {
  const [settingsSaved, setSettingsSaved] = useState<boolean>(false);

  const saveSettings = () => {
    try {
      localStorage.setItem('recorder-settings', JSON.stringify({ darkMode }));
      setSettingsSaved(true);
      setTimeout(() => setSettingsSaved(false), 3000);
    } catch (error) {
      console.error('Failed to save settings:', error);
      onError('保存设置失败');
    }
  };

  return (
    <div className="settings-tab">
      <div className="settings-container">
        {settingsSaved && (
          <div className="success-banner">
            <div className="success-content">
              <span className="success-icon">✓</span>
              <div className="success-text">
                <div className="success-title">设置已保存</div>
                <div className="success-message">配置已更新</div>
              </div>
              <button
                className="success-close"
                onClick={() => setSettingsSaved(false)}
              >
                ×
              </button>
            </div>
          </div>
        )}

        <h2 className="settings-title">设置</h2>

        <div className="settings-content">
          <div className="setting-item">
            <label className="setting-label">
              <input
                type="checkbox"
                checked={darkMode}
                onChange={(event) => setDarkMode(event.target.checked)}
              />
              <span className="checkbox-text">夜间模式</span>
            </label>
            <p className="setting-description">切换深色界面，适合夜间使用</p>
          </div>
        </div>

        <div className="settings-footer">
          <button
            className="settings-save-btn"
            onClick={saveSettings}
          >
            <Save size={16} />
            保存设置
          </button>
        </div>
      </div>
    </div>
  );
}
