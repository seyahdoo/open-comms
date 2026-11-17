/// <reference types="vite/client" />

const configuredOrigin = (import.meta.env.VITE_API_ORIGIN as string | undefined)?.trim();
const isTauri =
  typeof window !== 'undefined' && Object.prototype.hasOwnProperty.call(window, '__TAURI_INTERNALS__');

// The Tauri shell embeds the web app on a tauri:// origin, so API calls must target the server.
const defaultOrigin = isTauri ? 'http://localhost:8080' : '';

export const API_ORIGIN = (configuredOrigin || defaultOrigin).replace(/\/+$/, '');

export function apiUrl(path: string): string {
  const normalizedPath = path.startsWith('/') ? path : `/${path}`;
  return `${API_ORIGIN}${normalizedPath}`;
}

export function wsUrl(token: string): string {
  if (API_ORIGIN) {
    const url = new URL('/ws', API_ORIGIN);
    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
    url.searchParams.set('token', token);
    return url.toString();
  }

  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${window.location.host}/ws?token=${encodeURIComponent(token)}`;
}
