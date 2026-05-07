import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { ChevronRight, Clock, FileText, Loader2, RefreshCw, Sparkles, X } from 'lucide-react';
import { LocalJobPipelineStep, LocalQueueJob } from '../appTypes';
import './LocalJobPanel.css';

interface Props {
  workspaceFolder?: string;
  onError: (message: string) => void;
  onJobsChanged?: () => void | Promise<void>;
}

type Filter = 'all' | 'transcription' | 'summary';

export default function LocalJobPanel({
  workspaceFolder,
  onError,
  onJobsChanged,
}: Props) {
  const [jobs, setJobs] = useState<LocalQueueJob[]>([]);
  const [loading, setLoading] = useState(false);
  const [filter, setFilter] = useState<Filter>('all');
  const [expandedJobs, setExpandedJobs] = useState<Record<string, boolean>>({});

  const hasActiveJobs = useMemo(
    () => jobs.some(job => job.status === 'pending' || job.status === 'running'),
    [jobs]
  );

  const loadJobs = async () => {
    if (!workspaceFolder) {
      setJobs([]);
      return;
    }
    setLoading(true);
    try {
      const items = await invoke<LocalQueueJob[]>('list_local_queue_jobs', {
        workspaceFolder,
        queueType: filter === 'all' ? null : filter,
      });
      setJobs(items);
    } catch (error) {
      onError(typeof error === 'string' ? error : '加载本地任务失败');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadJobs();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspaceFolder, filter]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<LocalQueueJob>('local-queue-updated', event => {
      const job = event.payload;
      if (workspaceFolder && job.workspace_folder !== workspaceFolder) return;
      setJobs(prev => {
        const next = prev.filter(item => item.id !== job.id || item.queue_type !== job.queue_type);
        next.unshift(job);
        return next.sort((a, b) => {
          const left = a.updated_at || a.created_at;
          const right = b.updated_at || b.created_at;
          return right.localeCompare(left);
        });
      });
      void onJobsChanged?.();
    }).then(dispose => {
      unlisten = dispose;
    });
    return () => unlisten?.();
  }, [workspaceFolder, onJobsChanged]);

  useEffect(() => {
    if (!hasActiveJobs) return;
    const id = window.setInterval(() => {
      void loadJobs();
      void onJobsChanged?.();
    }, 2000);
    return () => window.clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasActiveJobs, workspaceFolder, filter, onJobsChanged]);

  const cancelJob = async (job: LocalQueueJob) => {
    try {
      const updated = await invoke<LocalQueueJob>('cancel_local_queue_job', {
        queueType: job.queue_type,
        jobId: job.id,
      });
      setJobs(prev => prev.map(item => item.id === updated.id && item.queue_type === updated.queue_type ? updated : item));
      void onJobsChanged?.();
    } catch (error) {
      onError(typeof error === 'string' ? error : '取消任务失败');
    }
  };

  const toggleJob = (job: LocalQueueJob) => {
    const key = jobKey(job);
    setExpandedJobs(prev => ({ ...prev, [key]: !prev[key] }));
  };

  return (
    <div className="local-job-panel">
      <div className="local-job-toolbar">
        <div className="local-job-filters">
          {(['all', 'transcription', 'summary'] as Filter[]).map(item => (
            <button
              key={item}
              className={`local-job-filter ${filter === item ? 'active' : ''}`}
              onClick={() => setFilter(item)}
            >
              {filterLabel(item)}
            </button>
          ))}
        </div>
        <button className="mini-btn" onClick={() => void loadJobs()} title="刷新任务">
          {loading ? <Loader2 size={16} className="spinning" /> : <RefreshCw size={16} />}
        </button>
      </div>

      {!jobs.length ? (
        <div className="local-job-empty">
          <Clock size={36} />
          <p>暂无任务</p>
        </div>
      ) : (
        <div className="local-job-list">
          {jobs.map(job => {
            const key = jobKey(job);
            const expanded = !!expandedJobs[key];
            const steps = job.metadata?.pipeline?.steps ?? [];
            return (
              <div key={key} className="local-job-item">
                <div className="local-job-header">
                  <button
                    className={`local-job-expand ${expanded ? 'open' : ''}`}
                    onClick={() => toggleJob(job)}
                    title={expanded ? '收起步骤' : '展开步骤'}
                    aria-label={expanded ? '收起步骤' : '展开步骤'}
                    aria-expanded={expanded}
                  >
                    <ChevronRight size={15} />
                  </button>
                  <div className="local-job-title">
                    {job.queue_type === 'transcription' ? <FileText size={16} /> : <Sparkles size={16} />}
                    <span title={job.title}>{job.title}</span>
                  </div>
                  <span className={`local-job-status ${statusTone(job.status)}`}>
                    {statusLabel(job.status)}
                  </span>
                </div>
                <div className="local-job-meta">
                  <span>{job.subtitle || queueLabel(job.queue_type)}</span>
                  <span>{formatTime(job.updated_at || job.created_at)}</span>
                </div>
                <div className="local-job-progress">
                  <div style={{ width: `${Math.max(0, Math.min(100, job.progress || 0))}%` }} />
                </div>
                {expanded && <PipelineSteps steps={steps} />}
                {job.error_message && <div className="local-job-error">{job.error_message}</div>}
                {job.status === 'pending' && (
                  <button className="local-job-cancel" onClick={() => void cancelJob(job)}>
                    <X size={13} />
                    取消
                  </button>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function PipelineSteps({ steps }: { steps: LocalJobPipelineStep[] }) {
  if (!steps.length) {
    return <div className="local-job-steps-empty">暂无步骤数据</div>;
  }
  return (
    <div className="local-job-steps">
      {steps.map((step, index) => (
        <PipelineStepItem key={`${step.name}:${index}`} step={step} />
      ))}
    </div>
  );
}

function PipelineStepItem({ step }: { step: LocalJobPipelineStep }) {
  const error = stepErrorMessage(step);
  return (
    <div className={`local-job-step ${stepTone(step.status)}`}>
      <span className="local-job-step-dot" />
      <div className="local-job-step-body">
        <div className="local-job-step-top">
          <span className="local-job-step-name">{step.display_name || step.name}</span>
          <span className="local-job-step-status">{stepStatusLabel(step.status)}</span>
        </div>
        <div className="local-job-step-meta">
          <span>{formatDuration(step.duration_ms)}</span>
          {typeof step.progress === 'number' && <span>{Math.max(0, Math.min(100, step.progress))}%</span>}
        </div>
        {error && <div className="local-job-step-error">{error}</div>}
      </div>
    </div>
  );
}

function jobKey(job: LocalQueueJob) {
  return `${job.queue_type}:${job.id}`;
}

function filterLabel(filter: Filter) {
  if (filter === 'transcription') return '转录';
  if (filter === 'summary') return '总结';
  return '全部';
}

function queueLabel(type: string) {
  return type === 'transcription' ? '转录任务' : '总结任务';
}

function statusLabel(status: string) {
  switch (status) {
    case 'pending': return '排队中';
    case 'running': return '处理中';
    case 'succeeded': return '完成';
    case 'failed': return '失败';
    case 'cancelled': return '已取消';
    case 'interrupted': return '已中断';
    default: return status;
  }
}

function statusTone(status: string) {
  if (status === 'succeeded') return 'ok';
  if (status === 'running' || status === 'pending') return 'warn';
  if (status === 'failed' || status === 'interrupted') return 'bad';
  return 'neutral';
}

function stepStatusLabel(status: string) {
  switch (status) {
    case 'pending': return '等待中';
    case 'running': return '执行中';
    case 'completed': return '已完成';
    case 'failed': return '失败';
    case 'skipped': return '已跳过';
    default: return status;
  }
}

function stepTone(status: string) {
  if (status === 'completed') return 'ok';
  if (status === 'running') return 'active';
  if (status === 'failed') return 'bad';
  if (status === 'skipped') return 'skip';
  return 'pending';
}

function formatTime(value?: string | null) {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function formatDuration(value?: number | null) {
  if (!value || value < 0) return '未计时';
  if (value < 1000) return `${value}ms`;
  const seconds = Math.floor(value / 1000);
  const minutes = Math.floor(seconds / 60);
  const remainSeconds = seconds % 60;
  if (minutes <= 0) return `${seconds}s`;
  return `${minutes}m ${remainSeconds}s`;
}

function stepErrorMessage(step: LocalJobPipelineStep) {
  const value = step.error?.message || step.metadata?.error;
  if (!value) return '';
  return typeof value === 'string' ? value : JSON.stringify(value);
}
