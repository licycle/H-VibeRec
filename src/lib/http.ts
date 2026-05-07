/**
 * Runtime-compatible HTTP fetch.
 *
 * The app can be opened in a browser during development and inside Tauri in
 * production. Browser fetch is used first; Tauri runtime can route through the
 * native HTTP plugin when the webview fetch path is unavailable.
 */
export async function httpFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  if (typeof globalThis.fetch === 'function') {
    try {
      return await globalThis.fetch(input, init);
    } catch (error) {
      if (!isTauriRuntime()) {
        throw error;
      }
      console.warn('Native fetch failed; using Tauri HTTP plugin:', error);
    }
  }

  try {
    const { fetch: tauriFetch } = await import('@tauri-apps/plugin-http');
    return (await tauriFetch(input.toString(), init as any)) as unknown as Response;
  } catch (error) {
    throw error instanceof Error ? error : new Error(String(error));
  }
}

function isTauriRuntime(): boolean {
  return Boolean(
    (globalThis as any).__TAURI__ ||
      (globalThis as any).__TAURI_INTERNALS__ ||
      (globalThis as any).window?.__TAURI__
  );
}
