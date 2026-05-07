import { useEffect, useState } from 'react';
import { ChevronDown, ChevronRight, Mic2 } from 'lucide-react';
import { VoiceInputStats } from '../appTypes';
import { useVoiceInputService } from '../hooks/useServices';
import './VoiceInputStatsPanel.css';

export default function VoiceInputStatsPanel() {
  const voiceInputService = useVoiceInputService();
  const [expanded, setExpanded] = useState(false);
  const [stats, setStats] = useState<VoiceInputStats | null>(null);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void voiceInputService.getStats()
      .then(nextStats => {
        if (!disposed) setStats(nextStats);
      })
      .catch(error => console.warn('Failed to load voice input stats', error));

    void voiceInputService.onStatsUpdated(nextStats => {
      if (!disposed) setStats(nextStats);
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
  }, [voiceInputService]);

  const displayStats = stats || emptyStats;

  return (
    <section className="voice-input-stats-panel" aria-label="语音输入法统计">
      <button
        type="button"
        className="voice-input-stats-summary"
        onClick={() => setExpanded(value => !value)}
      >
        {expanded ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
        <Mic2 size={15} />
        <span className="voice-input-stats-title">语音输入法</span>
      </button>

      <div className="voice-input-stats-primary">
        <Stat label="今日字数" value={formatNumber(displayStats.today_success_chars)} />
        <Stat label="今日次数" value={formatNumber(displayStats.today_success_count)} />
      </div>

      {expanded && (
        <div className="voice-input-stats-details">
          <Stat label="累计字数" value={formatNumber(displayStats.total_success_chars)} />
          <Stat label="累计次数" value={formatNumber(displayStats.total_success_count)} />
          <Stat label="最近一次" value={formatDateTime(displayStats.last_success_at)} />
          <Stat label="最近字数" value={formatNumber(displayStats.last_success_chars)} />
        </div>
      )}
    </section>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="voice-input-stat-card">
      <span className="voice-input-stat-label">{label}</span>
      <span className="voice-input-stat-value">{value}</span>
    </div>
  );
}

function formatNumber(value: number) {
  return new Intl.NumberFormat('zh-CN').format(value || 0);
}

function formatDateTime(value?: string | null) {
  if (!value) return '暂无';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

const emptyStats: VoiceInputStats = {
  today_success_count: 0,
  today_success_chars: 0,
  total_success_count: 0,
  total_success_chars: 0,
  last_success_at: null,
  last_success_chars: 0,
};
