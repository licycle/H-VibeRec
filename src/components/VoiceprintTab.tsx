import { Database } from 'lucide-react';

export function VoiceprintTab() {
  return (
    <div className="voiceprint-tab">
      <div className="empty-state">
        <Database size={48} />
        <h2>声纹库</h2>
        <p>声纹库功能正在开发中</p>
        <span>敬请期待</span>
      </div>
    </div>
  );
}