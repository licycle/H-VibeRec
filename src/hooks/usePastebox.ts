import { useCallback, useEffect, useRef, useState } from 'react';
import { pasteboxService } from '../services/pastebox.service';
import type { PasteItem, PasteboxState } from '../types/pastebox';
import { mergePasteItems } from '../lib/pastebox';

export function usePastebox() {
  const [state, setState] = useState<PasteboxState | null>(null);
  const [items, setItems] = useState<PasteItem[]>([]);
  const [query, setQuery] = useState('');
  const [pendingOnly, setPendingOnly] = useState(true);
  const [targetChoice, setTargetChoice] = useState('opening');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [hasMore, setHasMore] = useState(false);
  const generation = useRef(0);
  const actionLock = useRef(false);

  const refresh = useCallback(async (beforeSeq?: number) => {
    const request = ++generation.current;
    try {
      const next = await pasteboxService.getState({ beforeSeq, query, pendingOnly });
      if (request !== generation.current) return;
      setState(next);
      setItems(previous => mergePasteItems(previous, next.items, beforeSeq != null));
      setHasMore(next.items.length === 50);
    } catch (cause) {
      if (request === generation.current) setError(String(cause));
    } finally { if (request === generation.current) setLoading(false); }
  }, [query, pendingOnly]);

  useEffect(() => {
    let disposed = false;
    const cleanups: (() => void)[] = [];
    const subscribe = async () => {
      for (const registration of [pasteboxService.onChange(() => void refresh()), pasteboxService.onOpen(() => {
        setQuery(''); setPendingOnly(false); setError(null); setTargetChoice('opening'); void refresh();
      })]) {
        try { const cleanup = await registration; if (disposed) cleanup(); else cleanups.push(cleanup); }
        catch (cause) { if (!disposed) setError(String(cause)); }
      }
    };
    void subscribe();
    void refresh();
    return () => { disposed = true; generation.current++; cleanups.forEach(fn => fn()); };
  }, [refresh]);

  useEffect(() => {
    if (state?.requested_item_id) { setTargetChoice('original'); setPendingOnly(false); }
  }, [state?.requested_item_id]);
  useEffect(() => {
    if (state && !state.opening_target_id && targetChoice === 'opening') setTargetChoice('original');
  }, [state?.opening_target_id, targetChoice]);

  const run = useCallback(async (action: () => Promise<unknown>) => {
    if (actionLock.current) return;
    actionLock.current = true; setBusy(true); setError(null);
    try { await action(); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { actionLock.current = false; setBusy(false); await refresh(); }
  }, [refresh]);

  return { state, items, query, setQuery, pendingOnly, setPendingOnly, targetChoice, setTargetChoice,
    error, busy, loading, hasMore, refresh, run, service: pasteboxService };
}
