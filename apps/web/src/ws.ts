import type { WsServerEvent } from './types';

export type ConnectionStatus = 'connecting' | 'connected' | 'disconnected';

interface WsHandlers {
  onEvent: (event: WsServerEvent) => void;
  onStatus: (status: ConnectionStatus) => void;
}

export interface WsConnection {
  close(): void;
  isConnected(): boolean;
}

export function createWsConnection(token: string, handlers: WsHandlers): WsConnection {
  let stopped = false;
  let socket: WebSocket | null = null;
  let reconnectTimer: number | undefined;

  const connect = () => {
    if (stopped) return;

    handlers.onStatus('connecting');
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = `${protocol}//${window.location.host}/ws?token=${encodeURIComponent(token)}`;
    socket = new WebSocket(url);

    socket.onopen = () => handlers.onStatus('connected');

    socket.onmessage = (message) => {
      try {
        handlers.onEvent(JSON.parse(message.data as string) as WsServerEvent);
      } catch {
        // Ignore malformed events; the server controls the protocol shape.
      }
    };

    socket.onclose = () => {
      handlers.onStatus('disconnected');
      if (!stopped) {
        reconnectTimer = window.setTimeout(connect, 2000);
      }
    };

    socket.onerror = () => {
      socket?.close();
    };
  };

  connect();

  return {
    close() {
      stopped = true;
      if (reconnectTimer !== undefined) {
        window.clearTimeout(reconnectTimer);
      }
      socket?.close();
    },
    isConnected() {
      return socket?.readyState === WebSocket.OPEN;
    },
  };
}
