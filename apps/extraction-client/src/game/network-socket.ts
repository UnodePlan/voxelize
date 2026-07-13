const CONNECT_TIMEOUT_MS = 5_000;

export interface SocketOpenAttempt {
  opened: Promise<void>;
  socket: WebSocket;
}

export function openGameSocket(
  url: string,
  onMessage: (event: MessageEvent) => void,
  onClose: (socket: WebSocket, event: CloseEvent) => void,
): SocketOpenAttempt {
  const socket = new WebSocket(url);
  socket.binaryType = "arraybuffer";
  socket.onmessage = onMessage;
  const opened = new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => {
      socket.close();
      reject(new Error("WebSocket connection timed out"));
    }, CONNECT_TIMEOUT_MS);
    socket.onopen = () => {
      clearTimeout(timeout);
      resolve();
    };
    socket.onerror = () => {
      clearTimeout(timeout);
      socket.close();
      reject(new Error("WebSocket connection failed"));
    };
    socket.onclose = (event) => {
      clearTimeout(timeout);
      onClose(socket, event);
      reject(new Error("WebSocket closed before connecting"));
    };
  });
  return { opened, socket };
}
