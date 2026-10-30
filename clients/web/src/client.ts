import {
  CONTROL_CHANNEL,
  PROTO_VERSION,
  decodeControl,
  decodeFrame,
  encodeControl,
  encodeFrame,
} from "./protocol";
import type { TermColors } from "./queries";

interface AttachState {
  sessionId: number;
  cols: number;
  rows: number;
  lastSeq: number | null;
  colors: TermColors;
}

export class GateClient {
  private ws: WebSocket | null = null;
  private url = "";
  private token = "";
  private backoffMs = 250;
  private closed = false;
  private attachments = new Map<number, AttachState>();
  // xterm applies writes asynchronously. Keep controls and reconnect anchors behind
  // the parser callback, including frames received in the same browser turn.
  private receiving: Promise<void> = Promise.resolve();

  onControl: (msg: any) => void | Promise<void> = () => {};
  onChannelData: (channel: number, payload: Uint8Array) => void | Promise<void> = () => {};
  onOpen: () => void = () => {};
  onReconnecting: () => void = () => {};
  /** 认证失败等不可重试的错误:调用后连接不会再自动重连。 */
  onFatal: (message: string) => void = () => {};

  connect(url: string, token: string) {
    this.url = url;
    this.token = token;
    this.dial();
  }

  close() {
    this.closed = true;
    this.ws?.close();
  }

  private dial() {
    // Close events must see authentication facts immediately, even while an old
    // terminal write blocks receive dispatch. Each socket owns its own facts.
    let authOk = false;
    let preHelloError: string | null = null;
    let socketClosed = false;
    const ws = new WebSocket(this.url);
    ws.binaryType = "arraybuffer";
    this.ws = ws;
    ws.onopen = () => {
      ws.send(encodeControl({ type: "hello", token: this.token, proto: PROTO_VERSION }));
    };
    ws.onmessage = (ev) => {
      if (this.ws !== ws) return;
      const frame = decodeFrame(ev.data as ArrayBuffer);
      const msg = frame.channel === CONTROL_CHANNEL ? decodeControl(frame.payload) as any : null;
      if (msg?.type === "hello-ok") {
        authOk = true;
        this.backoffMs = 250;
      } else if (msg?.type === "error" && !authOk) {
        preHelloError = msg.message;
      }
      const channel = msg?.channel ?? frame.channel;
      const attachment = this.attachments.get(channel);
      if (channel !== CONTROL_CHANNEL && !attachment) return;
      this.receiving = this.receiving.then(async () => {
        if (msg?.type === "hello-ok" && socketClosed) return;
        if ((msg?.type === "hello-ok" || msg?.type === "error") && this.ws !== ws) return;
        // A switch can dispose the old terminal while its write is pending.
        if (channel !== CONTROL_CHANNEL && this.attachments.get(channel) !== attachment) return;
        if (msg !== null) {
          await this.handleControl(msg);
        } else {
          await this.onChannelData(channel, frame.payload);
          if (this.attachments.get(channel) === attachment) this.noteData(channel, frame.payload.length);
        }
      }).catch(err => console.error("gateterm: terminal receive failed", err));
    };
    ws.onclose = () => {
      socketClosed = true;
      if (this.ws !== ws) return;
      if (this.closed) return;
      // 这次拨号连 hello-ok 都没等到,而且服务端在那之前就已经报过错(典型情况:
      // token 不对)——重试没有意义,停止重连并让上层提示用户。
      if (!authOk && preHelloError !== null) {
        this.closed = true;
        try { localStorage.removeItem("gateterm-token"); } catch { /* Storage can be disabled. */ }
        this.onFatal(preHelloError);
        return;
      }
      this.onReconnecting();
      const delay = this.backoffMs;
      this.backoffMs = Math.min(this.backoffMs * 2, 10_000);
      setTimeout(() => this.dial(), delay);
    };
  }

  private async handleControl(msg: any) {
    const attachment = this.attachments.get(msg.channel);
    switch (msg.type) {
      case "hello-ok": {
        // 重连后恢复所有 attach
        for (const [channel, a] of this.attachments) {
          this.send({
            type: "attach",
            id: a.sessionId,
            channel,
            last_seq: a.lastSeq,
            cols: a.cols,
            rows: a.rows,
            colors: a.colors,
          });
        }
        this.onOpen();
        break;
      }
      case "attached":
        // Attached.seq 只是"重放结束点"的信息量,不是锚点:重放模式下 lastSeq
        // 已经等于本端发出的 last_seq(重连恢复点),后续 noteData 按字节推进即可。
        // 绝不能在这里用 msg.seq 覆盖 lastSeq —— 见 GateClient 类上方的 seq 记账不变式注释。
        break;
      case "resync-begin":
        // A partial synthetic snapshot is not a resumable byte-stream position.
        this.trackSeq(msg.channel, null);
        break;
    }
    await this.onControl(msg);
    if (msg.type === "resync-end" && this.attachments.get(msg.channel) === attachment) {
      this.trackSeq(msg.channel, msg.seq);
    }
  }

  private trackSeq(channel: number, seq: number | null) {
    const a = this.attachments.get(channel);
    if (a) a.lastSeq = seq;
  }

  send(msg: unknown) {
    this.ws?.send(encodeControl(msg));
  }

  sendInput(channel: number, bytes: Uint8Array) {
    this.ws?.send(encodeFrame(channel, bytes));
  }

  /** `colors` 是本端画这个会话用的前景/背景,服务端拿它应答 OSC 10/11(见 queries.ts)。 */
  attach(sessionId: number, channel: number, cols: number, rows: number, colors: TermColors) {
    this.attachments.set(channel, { sessionId, cols, rows, lastSeq: null, colors });
    this.send({ type: "attach", id: sessionId, channel, last_seq: null, cols, rows, colors });
  }

  /** 服务端 Data 帧没带 seq(裸字节)。lastSeq 只能从控制消息锚定:要么是
   *  Attached{mode:replay} 时本端自己发出的 last_seq(重放字节从该点续接,不
   *  需要、也不应该用 Attached.seq 去覆盖它),要么是 ResyncEnd.seq(resync 的
   *  重绘字节是合成的,不计入账本)。锚定之后,每收到 n 字节 lastSeq += n。 */
  noteData(channel: number, byteLen: number) {
    const a = this.attachments.get(channel);
    if (a && a.lastSeq !== null) a.lastSeq += byteLen;
  }

  detach(channel: number) {
    this.attachments.delete(channel);
    this.send({ type: "detach", channel });
  }

  /** 上报本端想要的尺寸(不是命令:权威尺寸由服务端归并所有 attach 后经
   *  `resized` 下发)。尺寸没变就不发——软键盘弹出/收起会连着触发一串 refit。 */
  resize(channel: number, cols: number, rows: number) {
    const a = this.attachments.get(channel);
    if (!a) return;
    if (a.cols === cols && a.rows === rows) return;
    a.cols = cols;
    a.rows = rows;
    this.send({ type: "resize", channel, cols, rows });
  }

  /** 换了主题。协议没有单独的"改颜色"消息(新增 client→server 类型要 bump
   *  proto),按 X4 搭在 resize 上,尺寸照旧。 */
  setColors(channel: number, colors: TermColors) {
    const a = this.attachments.get(channel);
    if (!a) return;
    if (a.colors.fg === colors.fg && a.colors.bg === colors.bg) return;
    a.colors = colors;
    this.send({ type: "resize", channel, cols: a.cols, rows: a.rows, colors });
  }
}
