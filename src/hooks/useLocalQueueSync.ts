import { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { LocalQueueJob, SummaryRecord, TranscriptRecord } from '../appTypes';
import { transcriptToMarkdown } from '../lib/transcriptMarkdown';

interface Options {
  workspaceFolder?: string;
  onTranscriptCreated?: (title: string, content: string) => unknown | Promise<unknown>;
  onWorkspaceSummaryCreated?: (title: string, content: string) => unknown | Promise<unknown>;
  onJobsChanged?: () => void | Promise<void>;
  onError?: (message: string) => void;
}

export function useLocalQueueSync({
  workspaceFolder,
  onTranscriptCreated,
  onWorkspaceSummaryCreated,
  onJobsChanged,
  onError,
}: Options) {
  const completedOutputIds = useRef(new Set<string>());
  const callbacksRef = useRef({
    onTranscriptCreated,
    onWorkspaceSummaryCreated,
    onJobsChanged,
    onError,
  });

  useEffect(() => {
    callbacksRef.current = {
      onTranscriptCreated,
      onWorkspaceSummaryCreated,
      onJobsChanged,
      onError,
    };
  }, [onTranscriptCreated, onWorkspaceSummaryCreated, onJobsChanged, onError]);

  useEffect(() => {
    const syncCompletedJob = async (job: LocalQueueJob): Promise<boolean> => {
      if (job.status !== 'succeeded') return false;
      if (workspaceFolder && job.workspace_folder !== workspaceFolder) return false;
      if (job.metadata?.frontend_synced_at) return false;

      const {
        onTranscriptCreated: createTranscriptNote,
        onWorkspaceSummaryCreated: createSummaryNote,
        onError: reportError,
      } = callbacksRef.current;

      if (job.queue_type === 'transcription' && job.output_transcript_id) {
        const marker = `transcript:${job.output_transcript_id}`;
        if (completedOutputIds.current.has(marker)) return false;
        completedOutputIds.current.add(marker);
        try {
          const transcript = await invoke<TranscriptRecord>('get_transcript', {
            transcriptId: job.output_transcript_id,
          });
          const title = job.subtitle ? `转录 - ${job.subtitle}` : '转录';
          await createTranscriptNote?.(title, transcriptToMarkdown(title, transcript.text));
          await invoke<LocalQueueJob>('mark_local_queue_job_synced', {
            queueType: job.queue_type,
            jobId: job.id,
          });
          return true;
        } catch (error) {
          reportError?.(typeof error === 'string' ? error : '读取转录结果失败');
          return false;
        }
      }

      if (job.queue_type === 'summary' && job.output_summary_id) {
        const marker = `summary:${job.output_summary_id}`;
        if (completedOutputIds.current.has(marker)) return false;
        completedOutputIds.current.add(marker);
        try {
          const summary = await invoke<SummaryRecord>('get_summary', {
            summaryId: job.output_summary_id,
          });
          const title = summary.title || job.title;
          await createSummaryNote?.(title, `# ${title}\n\n${summary.content.trim()}`);
          await invoke<LocalQueueJob>('mark_local_queue_job_synced', {
            queueType: job.queue_type,
            jobId: job.id,
          });
          return true;
        } catch (error) {
          reportError?.(typeof error === 'string' ? error : '读取总结结果失败');
          return false;
        }
      }
      return false;
    };

    const scanCompletedJobs = async () => {
      if (!workspaceFolder) return;
      try {
        const jobs = await invoke<LocalQueueJob[]>('list_local_queue_jobs', {
          workspaceFolder,
          queueType: null,
        });
        const synced = await Promise.all(jobs.map(syncCompletedJob));
        if (synced.some(Boolean)) {
          await callbacksRef.current.onJobsChanged?.();
        }
      } catch (error) {
        callbacksRef.current.onError?.(typeof error === 'string' ? error : '同步已完成任务失败');
      }
    };

    let unlisten: (() => void) | undefined;
    void scanCompletedJobs();
    void listen<LocalQueueJob>('local-queue-updated', event => {
      void syncCompletedJob(event.payload).then(synced => {
        if (synced || event.payload.status === 'succeeded') {
          void callbacksRef.current.onJobsChanged?.();
        }
      });
    }).then(dispose => {
      unlisten = dispose;
    });
    return () => unlisten?.();
  }, [workspaceFolder]);
}
