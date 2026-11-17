import './styles.css';
import { api, getToken, setToken } from './api';
import type {
  Channel,
  ChannelType,
  Message,
  MessageReply,
  User,
  WsServerEvent,
} from './types';
import { createWsConnection, type ConnectionStatus, type WsConnection } from './ws';

let token: string | null = getToken();
let currentUser: User | null = null;
let channels: Channel[] = [];
let selectedChannel: Channel | null = null;
let messages: Message[] = [];
let socketConnection: WsConnection | null = null;
const expandedReplies = new Set<string>();

function qs<T extends HTMLElement>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`Missing element: ${selector}`);
  return element;
}

function boot(): void {
  renderShell();
  bindEvents();
  if (token) {
    void restoreSession(token);
  } else {
    showLogin();
  }
}

function renderShell(): void {
  qs<HTMLDivElement>('#root').innerHTML = `
    <div id="loginView" class="login-view">
      <form id="loginForm" class="login-card">
        <div class="brand-mark">Open Comms</div>
        <h1>Sign in</h1>
        <p class="muted">Enter a username to join. Sessions are created automatically.</p>
        <input id="usernameInput" type="text" placeholder="Username" autocomplete="username" required />
        <button class="button primary" type="submit">Join</button>
        <p id="loginError" class="error"></p>
      </form>
    </div>

    <div id="appView" class="app-view hidden">
      <header class="topbar">
        <div class="brand-mark">Open Comms</div>
        <div class="top-actions">
          <span id="statusBadge" class="status status-disconnected">disconnected</span>
          <span id="userLabel" class="user-label"></span>
          <button id="logoutBtn" class="button ghost" type="button">Log out</button>
        </div>
      </header>
      <div class="layout">
        <aside class="sidebar">
          <div class="section-title">Channels</div>
          <div id="channelList" class="channel-list"></div>
          <form id="channelForm" class="channel-form">
            <input id="channelNameInput" type="text" placeholder="New channel" required />
            <select id="channelTypeInput" aria-label="Channel type">
              <option value="text">Text</option>
              <option value="voice">Voice</option>
            </select>
            <button class="button" type="submit">Create</button>
          </form>
        </aside>
        <main class="chat">
          <div class="channel-header">
            <strong id="channelTitle">Select a channel</strong>
          </div>
          <div id="messageList" class="message-list"></div>
          <form id="messageForm" class="message-form">
            <textarea id="messageInput" rows="1" placeholder="Write a message..."></textarea>
            <button class="button primary" type="submit">Send</button>
          </form>
        </main>
      </div>
    </div>

    <div id="toast" class="toast" hidden></div>
  `;
}

function bindEvents(): void {
  qs<HTMLFormElement>('#loginForm').addEventListener('submit', (event) => {
    event.preventDefault();
    void handleLogin();
  });

  qs<HTMLFormElement>('#channelForm').addEventListener('submit', (event) => {
    event.preventDefault();
    void handleCreateChannel();
  });

  qs<HTMLFormElement>('#messageForm').addEventListener('submit', (event) => {
    event.preventDefault();
    void handleSendMessage();
  });

  qs<HTMLTextAreaElement>('#messageInput').addEventListener('keydown', (event) => {
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      void handleSendMessage();
    }
  });

  qs<HTMLButtonElement>('#logoutBtn').addEventListener('click', handleLogout);
}

async function handleLogin(): Promise<void> {
  const username = qs<HTMLInputElement>('#usernameInput').value.trim();
  const error = qs<HTMLParagraphElement>('#loginError');
  error.textContent = '';

  if (!username) return;

  try {
    const response = await api.register(username);
    setToken(response.token);
    currentUser = response.user;
    await startSession(response.token);
  } catch (err) {
    error.textContent = errorMessage(err);
  }
}

async function restoreSession(sessionToken: string): Promise<void> {
  try {
    const user = await api.me();
    if (!user) throw new Error('Session expired');
    currentUser = user;
    await startSession(sessionToken);
  } catch {
    setToken(null);
    token = null;
    currentUser = null;
    showLogin();
  }
}

async function startSession(sessionToken: string): Promise<void> {
  token = sessionToken;
  showApp();
  qs<HTMLSpanElement>('#userLabel').textContent = currentUser?.username ?? '';

  socketConnection?.close();
  socketConnection = createWsConnection(sessionToken, {
    onEvent: handleWsEvent,
    onStatus: setStatus,
  });

  await loadChannels();
}

function handleLogout(): void {
  socketConnection?.close();
  socketConnection = null;
  setToken(null);
  token = null;
  currentUser = null;
  channels = [];
  selectedChannel = null;
  messages = [];
  showLogin();
}

function showLogin(): void {
  qs('#loginView').classList.remove('hidden');
  qs('#appView').classList.add('hidden');
}

function showApp(): void {
  qs('#loginView').classList.add('hidden');
  qs('#appView').classList.remove('hidden');
}

function setStatus(status: ConnectionStatus): void {
  const badge = qs<HTMLSpanElement>('#statusBadge');
  badge.textContent = status;
  badge.className = `status status-${status}`;
}

async function loadChannels(): Promise<void> {
  try {
    channels = await api.listChannels();
    renderChannels();

    if (!selectedChannel) {
      const firstText = channels.find((channel) => channel.channel_type === 'text');
      if (firstText) await selectChannel(firstText);
    }
  } catch (err) {
    toast(errorMessage(err), true);
  }
}

function renderChannels(): void {
  const list = qs<HTMLDivElement>('#channelList');
  list.replaceChildren();

  for (const channel of channels) {
    const label = channel.channel_type === 'voice' ? `🔊 ${channel.name}` : `# ${channel.name}`;
    const button = createButton(
      label,
      () => void selectChannel(channel),
      selectedChannel?.id === channel.id ? 'channel-item selected' : 'channel-item',
    );
    list.appendChild(button);
  }
}

async function selectChannel(channel: Channel): Promise<void> {
  selectedChannel = channel;
  expandedReplies.clear();
  renderChannels();
  qs<HTMLElement>('#channelTitle').textContent =
    channel.channel_type === 'voice' ? `🔊 ${channel.name}` : `# ${channel.name}`;

  const isText = channel.channel_type === 'text';
  qs<HTMLFormElement>('#messageForm').classList.toggle('hidden', !isText);

  if (!isText) {
    messages = [];
    const list = qs<HTMLDivElement>('#messageList');
    list.replaceChildren();
    list.appendChild(createEmpty('Voice channels will use WebRTC when implemented.'));
    return;
  }

  try {
    messages = await api.listMessages(channel.id);
    renderMessages();
  } catch (err) {
    toast(errorMessage(err), true);
  }
}

async function handleCreateChannel(): Promise<void> {
  const name = qs<HTMLInputElement>('#channelNameInput').value.trim();
  const channelType = qs<HTMLSelectElement>('#channelTypeInput').value as ChannelType;
  if (!name) return;

  try {
    const channel = await api.createChannel(name, channelType);
    upsertChannel(channel);
    qs<HTMLInputElement>('#channelNameInput').value = '';
    await selectChannel(channel);
  } catch (err) {
    toast(errorMessage(err), true);
  }
}

async function handleSendMessage(): Promise<void> {
  const input = qs<HTMLTextAreaElement>('#messageInput');
  const body = input.value.trim();
  if (!body || !selectedChannel || selectedChannel.channel_type !== 'text') return;

  try {
    const message = await api.createMessage(selectedChannel.id, body);
    input.value = '';
    upsertMessage(message);
  } catch (err) {
    toast(errorMessage(err), true);
  }
}

function handleWsEvent(event: WsServerEvent): void {
  switch (event.type) {
    case 'channel_created':
      upsertChannel(event.payload);
      break;
    case 'message_created':
      if (selectedChannel?.id === event.payload.channel_id) {
        upsertMessage(event.payload);
      }
      break;
    case 'reply_created':
      if (selectedChannel?.id === event.payload.channel_id) {
        const root = messages.find((message) => message.id === event.payload.root_message_id);
        if (root && !containsReply(root.replies, event.payload.reply.id)) {
          if (root.id === event.payload.reply.parent_id) {
            root.replies.push(event.payload.reply);
          } else {
            insertReply(root.replies, event.payload.reply.parent_id, event.payload.reply);
          }
          renderMessages();
        }
      }
      break;
    case 'reaction_updated': {
      if (selectedChannel?.id !== event.payload.channel_id) break;
      const root = messages.find(
        (message) =>
          message.id === event.payload.message_id ||
          containsReply(message.replies, event.payload.message_id),
      );
      if (!root) break;
      if (root.id === event.payload.message_id) {
        root.reactions = event.payload.reactions;
      } else {
        setReplyReactions(root.replies, event.payload.message_id, event.payload.reactions);
      }
      renderMessages();
      break;
    }
    case 'error':
      toast(event.payload.message, true);
      break;
    case 'pong':
      break;
  }
}

function upsertChannel(channel: Channel): void {
  const index = channels.findIndex((item) => item.id === channel.id);
  if (index >= 0) {
    channels[index] = channel;
  } else {
    channels.push(channel);
  }
  renderChannels();
}

function upsertMessage(message: Message): void {
  const index = messages.findIndex((item) => item.id === message.id);
  if (index >= 0) {
    messages[index] = message;
  } else {
    messages.push(message);
  }
  messages.sort((a, b) => a.created_at.localeCompare(b.created_at));
  renderMessages();
}

function insertReply(
  replies: MessageReply[],
  parentId: string,
  reply: MessageReply,
): boolean {
  const parent = replies.find((item) => item.id === parentId);
  if (parent) {
    if (!containsReply(parent.replies, reply.id)) parent.replies.push(reply);
    return true;
  }
  for (const item of replies) {
    if (insertReply(item.replies, parentId, reply)) return true;
  }
  return false;
}

function containsReply(replies: MessageReply[], id: string): boolean {
  return replies.some((reply) => reply.id === id || containsReply(reply.replies, id));
}

function setReplyReactions(
  replies: MessageReply[],
  id: string,
  reactions: MessageReply['reactions'],
): boolean {
  for (const reply of replies) {
    if (reply.id === id) {
      reply.reactions = reactions;
      return true;
    }
    if (setReplyReactions(reply.replies, id, reactions)) return true;
  }
  return false;
}

function renderMessages(): void {
  const list = qs<HTMLDivElement>('#messageList');
  list.replaceChildren();

  if (messages.length === 0) {
    list.appendChild(createEmpty('No messages yet. Start the conversation.'));
    return;
  }

  for (const message of messages) {
    list.appendChild(renderMessage(message));
  }

  requestAnimationFrame(() => {
    list.scrollTop = list.scrollHeight;
  });
}

function renderMessage(item: Message | MessageReply): HTMLElement {
  const article = document.createElement('article');
  article.className = 'message';

  const header = document.createElement('div');
  header.className = 'message-header';
  const author = document.createElement('strong');
  author.textContent = shortId(item.author_id);
  const time = document.createElement('time');
  time.textContent = formatTime(item.created_at);
  header.append(author, time);

  const body = document.createElement('div');
  body.className = 'message-body';
  body.textContent = item.body;

  const actions = document.createElement('div');
  actions.className = 'message-actions';
  actions.appendChild(createButton('Reply', () => showReplyForm(article, item.id), 'text-button'));
  actions.appendChild(createButton('React 👍', () => void react(item.id, '👍'), 'text-button'));

  const replyCount = countReplies(item.replies);
  if (replyCount > 0) {
    actions.appendChild(
      createButton(`Replies (${replyCount})`, () => toggleReplies(item.id), 'text-button'),
    );
  }

  const replies = document.createElement('div');
  replies.className = 'replies';
  replies.dataset.repliesFor = item.id;
  if (!expandedReplies.has(item.id)) replies.classList.add('hidden');
  for (const reply of item.replies) {
    replies.appendChild(renderMessage(reply));
  }

  article.append(header, body, renderReactions(item), actions, replies);
  return article;
}

function renderReactions(item: Message | MessageReply): HTMLElement {
  const container = document.createElement('div');
  container.className = 'reactions';
  for (const reaction of item.reactions) {
    container.appendChild(
      createButton(
        `${reaction.emoji} ${reaction.user_ids.length}`,
        () => void react(item.id, reaction.emoji),
        'reaction',
      ),
    );
  }
  return container;
}

function showReplyForm(article: HTMLElement, targetId: string): void {
  const existing = article.querySelector<HTMLFormElement>(':scope > .inline-form');
  if (existing) {
    existing.remove();
    return;
  }

  const form = document.createElement('form');
  form.className = 'inline-form';

  const input = document.createElement('textarea');
  input.rows = 1;
  input.placeholder = 'Write a reply...';

  const send = createSubmitButton('Send');
  const cancel = createButton('Cancel', () => form.remove(), 'text-button');

  form.addEventListener('submit', (event) => {
    event.preventDefault();
    const body = input.value.trim();
    if (!body || !selectedChannel) return;
    void (async () => {
      try {
        await api.createReply(selectedChannel.id, targetId, body);
        form.remove();
        await reloadMessages();
      } catch (err) {
        toast(errorMessage(err), true);
      }
    })();
  });

  form.append(input, send, cancel);
  article.appendChild(form);
}

async function react(targetId: string, emoji: string): Promise<void> {
  if (!selectedChannel) return;
  try {
    await api.addReaction(selectedChannel.id, targetId, emoji);
    await reloadMessages();
  } catch (err) {
    toast(errorMessage(err), true);
  }
}

async function reloadMessages(): Promise<void> {
  if (!selectedChannel || selectedChannel.channel_type !== 'text') return;
  try {
    messages = await api.listMessages(selectedChannel.id);
    renderMessages();
  } catch (err) {
    toast(errorMessage(err), true);
  }
}

function toggleReplies(id: string): void {
  if (expandedReplies.has(id)) {
    expandedReplies.delete(id);
  } else {
    expandedReplies.add(id);
  }
  renderMessages();
}

function countReplies(replies: MessageReply[]): number {
  return replies.reduce((total, reply) => total + 1 + countReplies(reply.replies), 0);
}

function createEmpty(text: string): HTMLElement {
  const element = document.createElement('div');
  element.className = 'empty';
  element.textContent = text;
  return element;
}

function createButton(label: string, onClick: () => void, className = 'button'): HTMLButtonElement {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = className;
  button.textContent = label;
  button.addEventListener('click', onClick);
  return button;
}

function createSubmitButton(label: string): HTMLButtonElement {
  const button = document.createElement('button');
  button.type = 'submit';
  button.className = 'button primary small';
  button.textContent = label;
  return button;
}

function shortId(id: string): string {
  return id.slice(0, 8);
}

function formatTime(value: string): string {
  return new Date(value).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : 'Something went wrong';
}

let toastTimer: number | undefined;
function toast(message: string, isError = false): void {
  const element = qs<HTMLDivElement>('#toast');
  element.textContent = message;
  element.className = isError ? 'toast toast-error' : 'toast';
  element.hidden = false;
  if (toastTimer !== undefined) window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    element.hidden = true;
  }, 3500);
}

void boot();
