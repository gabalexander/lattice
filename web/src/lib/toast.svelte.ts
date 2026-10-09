// A line at the bottom of the window saying what just happened, for a few seconds.

class Toast {
  text = $state('');
  private timer: ReturnType<typeof setTimeout> | undefined;

  show(text: string) {
    this.text = text;
    clearTimeout(this.timer);
    this.timer = setTimeout(() => (this.text = ''), 2800);
  }
}

export const toast = new Toast();

/** Copies `text`, the old way when the clipboard API isn't there (an insecure context). */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // Not a secure context, or no permission.
  }
  const area = document.createElement('textarea');
  area.value = text;
  area.setAttribute('readonly', '');
  area.style.cssText = 'position:fixed;top:0;left:0;opacity:0';
  document.body.appendChild(area);
  area.select();
  let ok = false;
  try {
    ok = document.execCommand('copy');
  } catch {
    ok = false;
  }
  area.remove();
  return ok;
}
