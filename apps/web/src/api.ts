import type {
  AuthResponse,
  Channel,
  ChannelType,
  Message,
  Reaction,
  ReplyResponse,
  User,
} from './types';

const TOKEN_KEY = 'open-comms-token';

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string | null): void {
  if (token) {
    localStorage.setItem(TOKEN_KEY, token);
  } else {
    localStorage.removeItem(TOKEN_KEY);
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  const token = getToken();

  if (token) headers.set('Authorization', `Bearer ${token}`);
  if (init.body && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json');
  }

  const response = await fetch(path, { ...init, headers });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `Request failed with ${response.status}`);
  }

  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

function jsonBody(value: unknown): string {
  return JSON.stringify(value);
}

export const api = {
  register(username: string): Promise<AuthResponse> {
    return request('/auth/register', {
      method: 'POST',
      body: jsonBody({ username }),
    });
  },

  me(): Promise<User | null> {
    return request<User | null>('/auth/me');
  },

  listChannels(): Promise<Channel[]> {
    return request<Channel[]>('/channels');
  },

  createChannel(name: string, channelType: ChannelType): Promise<Channel> {
    return request<Channel>('/channels', {
      method: 'POST',
      body: jsonBody({ name, channel_type: channelType }),
    });
  },

  listMessages(channelId: string, limit = 100): Promise<Message[]> {
    return request<Message[]>(`/channels/${channelId}/messages?limit=${limit}`);
  },

  createMessage(channelId: string, body: string): Promise<Message> {
    return request<Message>(`/channels/${channelId}/messages`, {
      method: 'POST',
      body: jsonBody({ body }),
    });
  },

  createReply(channelId: string, parentId: string, body: string): Promise<ReplyResponse> {
    return request<ReplyResponse>(
      `/channels/${channelId}/messages/${parentId}/replies`,
      {
        method: 'POST',
        body: jsonBody({ body }),
      },
    );
  },

  addReaction(
    channelId: string,
    messageId: string,
    emoji: string,
  ): Promise<{ message_id: string; reactions: Reaction[] }> {
    return request(`/channels/${channelId}/messages/${messageId}/reactions`, {
      method: 'POST',
      body: jsonBody({ emoji }),
    });
  },
};
