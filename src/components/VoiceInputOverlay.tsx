import { useEffect, useState, type MouseEvent, type PointerEvent } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Check, CheckCircle2, GripHorizontal, Loader2, Mic2, Wand2, X, XCircle } from 'lucide-react';
import { VoiceInputStatusEvent } from '../appTypes';
import { useVoiceInputService } from '../hooks/useServices';
import './VoiceInputOverlay.css';

const visiblePhases = new Set(['starting', 'listening', 'preparing_model', 'transcribing', 'refining', 'inserting', 'inserted', 'copied', 'failed', 'cancelled']);
const finalPhases = new Set(['inserted', 'copied', 'failed', 'cancelled']);
interface VoiceInputOverlayProps {
  standalone?: boolean;
}

export default function VoiceInputOverlay({ standalone = false }: VoiceInputOverlayProps) {
  const voiceInputService = useVoiceInputService();
  const [event, setEvent] = useState<VoiceInputStatusEvent | null>(null);
  const [busyAction, setBusyAction] = useState<'cancel' | 'confirm' | null>(null);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void voiceInputService.getStatus().then(status => {
      if (disposed || !visiblePhases.has(status.phase)) return;
      setEvent({
        phase: status.phase,
        message: status.message,
      });
    }).catch(() => undefined);

    void voiceInputService.onStatus(nextEvent => {
      if (disposed || !visiblePhases.has(nextEvent.phase)) return;
      setEvent(nextEvent);
      setBusyAction(null);
      if (finalPhases.has(nextEvent.phase)) {
        window.setTimeout(() => {
          if (!disposed) setEvent(current => (current === nextEvent ? null : current));
        }, 3200);
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
  }, [voiceInputService]);

  async function handleDragStart(event: PointerEvent<HTMLButtonElement>) {
    event.stopPropagation();
    event.preventDefault();
    try {
      await getCurrentWindow().startDragging();
    } catch (error) {
      console.warn('Failed to start voice input overlay drag', error);
    }
  }

  async function handleOpenMain(event: MouseEvent<HTMLButtonElement>) {
    event.stopPropagation();

    try {
      await voiceInputService.openMainWindow();
    } catch (error) {
      console.warn('Failed to open main window from voice input overlay', error);
    }
  }

  async function handleCancel(event: MouseEvent<HTMLButtonElement>) {
    event.stopPropagation();
    if (busyAction) return;
    setBusyAction('cancel');
    try {
      await voiceInputService.cancelDictation();
    } catch (error) {
      setBusyAction(null);
      setEvent({
        phase: 'failed',
        message: errorMessage(error, '取消语音输入失败'),
      });
    }
  }

  async function handleConfirm(event: MouseEvent<HTMLButtonElement>) {
    event.stopPropagation();
    if (busyAction) return;
    setBusyAction('confirm');
    try {
      await voiceInputService.stopDictation();
    } catch (error) {
      setBusyAction(null);
      setEvent({
        phase: 'failed',
        message: errorMessage(error, '语音输入失败'),
      });
    }
  }

  if (!event) return null;

  const listening = event.phase === 'listening';
  const controlsDisabled = !!busyAction;

  return (
    <div className={`voice-input-overlay ${standalone ? 'standalone' : ''}`} aria-live="polite">
      <div className={`voice-input-overlay-pill ${event.phase}`}>
        <button
          type="button"
          className="voice-input-overlay-drag-handle"
          aria-label="拖动控件"
          title="拖动控件"
          onPointerDown={event => void handleDragStart(event)}
          onClick={stopOverlayDrag}
        >
          <GripHorizontal size={12} />
        </button>
        {listening && (
          <button
            type="button"
            className="voice-input-overlay-action cancel"
            aria-label="取消语音输入"
            title="取消语音输入"
            disabled={controlsDisabled}
            onMouseDown={stopOverlayDrag}
            onMouseUp={stopOverlayDrag}
            onClick={event => void handleCancel(event)}
          >
            <X size={14} />
          </button>
        )}
        <span className="voice-input-overlay-state-icon">{iconForPhase(event.phase)}</span>
        <button
          type="button"
          className="voice-input-overlay-open-target"
          title="打开主控件"
          onClick={event => void handleOpenMain(event)}
        >
          <span className="voice-input-overlay-message">{event.message || labelForPhase(event.phase)}</span>
          {event.char_count != null && <span className="voice-input-overlay-count">{event.char_count} 字</span>}
        </button>
        {listening && (
          <button
            type="button"
            className="voice-input-overlay-action confirm"
            aria-label="确认语音输入"
            title="确认语音输入"
            disabled={controlsDisabled}
            onMouseDown={stopOverlayDrag}
            onMouseUp={stopOverlayDrag}
            onClick={event => void handleConfirm(event)}
          >
            <Check size={14} />
          </button>
        )}
      </div>
    </div>
  );
}

function stopOverlayDrag(event: MouseEvent<HTMLElement>) {
  event.stopPropagation();
}

function iconForPhase(phase: string) {
  if (phase === 'inserted' || phase === 'copied') return <CheckCircle2 size={15} />;
  if (phase === 'failed') return <XCircle size={15} />;
  if (phase === 'refining') return <Wand2 size={15} />;
  if (phase === 'listening') return <Mic2 size={15} />;
  if (phase === 'cancelled') return <XCircle size={15} />;
  return <Loader2 size={15} className="spinning" />;
}

function labelForPhase(phase: string) {
  switch (phase) {
    case 'listening':
      return '正在听写';
    case 'starting':
      return '麦克风启动中，请稍候';
    case 'preparing_model':
      return '正在准备 ASR 模型';
    case 'transcribing':
      return '正在转写';
    case 'refining':
      return '正在润色';
    case 'inserting':
      return '正在写入';
    case 'inserted':
      return '已写入';
    case 'copied':
      return '已复制';
    case 'failed':
      return '语音输入失败';
    case 'cancelled':
      return '已取消';
    default:
      return '语音输入法';
  }
}

function errorMessage(error: unknown, fallback: string) {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return fallback;
}
