export const TOKEN_ENV = "LOOM_TOKEN";

export interface Thread {
  color: string;
  length: number;
}

export enum Weave {
  Plain,
  Twill = "twill",
}

export class Loom {
  private threads: Thread[] = [];

  weave(thread: Thread): void {
    this.threads.push(thread);
  }

  static create(): Loom {
    return new Loom();
  }
}

export const spin = (fiber: string): Thread => ({ color: fiber, length: 1 });

export function cut(thread: Thread): Thread {
  return thread;
}
