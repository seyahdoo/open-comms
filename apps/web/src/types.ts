export type ChannelType = 'text' | 'voice';

export interface Channel {
  id: string;
  name: string;
  channel_type: ChannelType;
  created_at: string;
}

export interface User {
  id: string;
  username: string;
  created_at: string;
}

export interface AuthResponse {
  user: User;
  token: string;
}

export interface Reaction {
  emoji: string;
  user_ids: string[];
}

export interface MessageReply {
  id: string;
  parent_id: string;
  author_id: string;
  body: string;
  created_at: string;
  edited_at?: string | null;
  replies: MessageReply[];
  reactions: Reaction[];
}

export interface Message {
  id: string;
  channel_id: string;
  author_id: string;
  body: string;
  created_at: string;
  edited_at?: string | null;
  replies: MessageReply[];
  reactions: Reaction[];
}

export interface ReplyResponse {
  root_message_id: string;
  reply: MessageReply;
}

export type WsServerEvent =
  | { type: 'pong' }
  | { type: 'error'; payload: { message: string } }
  | { type: 'channel_created'; payload: Channel }
  | { type: 'message_created'; payload: Message }
  | {
      type: 'reply_created';
      payload: {
        channel_id: string;
        root_message_id: string;
        reply: MessageReply;
      };
    }
  | {
      type: 'reaction_updated';
      payload: {
        channel_id: string;
        message_id: string;
        reactions: Reaction[];
      };
    };
