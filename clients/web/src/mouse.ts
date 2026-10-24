import type { Terminal } from "@xterm/xterm";

export interface MouseEventPolicy {
  setBlocked(blocked: boolean): void;
  dispose(): void;
}

/**
 * xterm 5.5 has no public option to override application mouse tracking. Merely
 * dropping onData bytes would still leave local selection disabled. Intercept
 * this one instance's protocol setter so the effective protocol stays NONE,
 * enabling xterm's normal selection handlers without changing terminal output.
 *
 * Remember application requests (including disables and resets) while blocked,
 * so unchecking restores its latest mode immediately. Keep this adapter covered
 * by browser checks when changing the pinned xterm version.
 */
export function installMouseEventPolicy(terminal: Terminal, initiallyBlocked = false): MouseEventPolicy {
  const mouse = (terminal as unknown as {
    _core: { coreMouseService: { activeProtocol: string } };
  })._core.coreMouseService;
  const own = Object.getOwnPropertyDescriptor(mouse, "activeProtocol");
  const protocol = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(mouse), "activeProtocol");
  if (!protocol?.get || !protocol.set) throw new Error("Unsupported xterm mouse service");
  const write = (name: string) => protocol.set!.call(mouse, name);
  let requested = mouse.activeProtocol;
  let blocked = initiallyBlocked;
  Object.defineProperty(mouse, "activeProtocol", {
    configurable: true,
    get: () => requested,
    set: (name: string) => {
      requested = name;
      write(blocked ? "NONE" : name);
    },
  });
  write(blocked ? "NONE" : requested);

  terminal.attachCustomWheelEventHandler((event) => {
    if (!blocked) return true;
    // With normal-buffer scrollback, xterm scrolls locally. Otherwise it sends
    // arrow keys instead (not mouse reports); suppress those as well.
    if (terminal.buffer.active.type === "normal" && (terminal.options.scrollback ?? 0) > 0) return true;
    event.preventDefault();
    return false;
  });

  return {
    setBlocked(value) {
      if (blocked === value) return;
      blocked = value;
      write(blocked ? "NONE" : requested);
    },
    dispose() {
      if (own) Object.defineProperty(mouse, "activeProtocol", own);
      else delete (mouse as { activeProtocol?: string }).activeProtocol;
      write(requested);
      terminal.attachCustomWheelEventHandler(() => true);
    },
  };
}
