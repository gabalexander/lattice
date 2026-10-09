<!-- A diagram large, in a modal filling most of the window, as Code Wiki's zoom opens it: dragged to move,
     ⌘ or Ctrl and the wheel (or a pinch) to zoom, + − 0 and the arrows from the keyboard; Esc or the close
     button closes it. -->
<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '$lib/components/Icon.svelte';
  import { enlarged, type Drawing } from '$lib/mermaid';

  let { drawing, caption, onclose }: { drawing: Drawing; caption: string; onclose: () => void } = $props();

  let dialog: HTMLDialogElement;
  let stage: HTMLElement;
  let x = $state(0);
  let y = $state(0);
  let k = $state(1);
  let dragging = $state(false);
  const pointers = new Map<number, { x: number; y: number }>();
  let pinch: { d: number; k: number } | null = null;
  const svg = $derived(enlarged(drawing));

  const phone = () => innerWidth < 840;

  function fit() {
    const rect = stage.getBoundingClientRect();
    const top = phone() ? 64 : 84;
    k = Math.max(0.05, Math.min((rect.width - 48) / drawing.width, (rect.height - top - 32) / drawing.height, 2.5));
    x = (rect.width - drawing.width * k) / 2;
    y = top + (rect.height - top - 32 - drawing.height * k) / 2;
  }

  function zoomAt(factor: number, cx: number, cy: number) {
    const next = Math.min(10, Math.max(0.05, k * factor));
    x = cx - (cx - x) * (next / k);
    y = cy - (cy - y) * (next / k);
    k = next;
  }

  function zoomCentre(factor: number) {
    const rect = stage.getBoundingClientRect();
    zoomAt(factor, rect.width / 2, rect.height / 2);
  }

  onMount(() => {
    dialog.showModal();
    fit();
    // The wheel zooms or pans here rather than scrolling the page under the dialog, so it can't be passive.
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      const rect = stage.getBoundingClientRect();
      if (event.ctrlKey || event.metaKey) {
        const factor = Math.min(1.25, Math.max(0.8, Math.exp(-event.deltaY * (event.deltaMode ? 0.05 : 0.01))));
        zoomAt(factor, event.clientX - rect.left, event.clientY - rect.top);
      } else {
        const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? rect.height : 1;
        x -= event.deltaX * unit;
        y -= event.deltaY * unit;
      }
    };
    stage.addEventListener('wheel', wheel, { passive: false });
    return () => stage.removeEventListener('wheel', wheel);
  });

  function onKey(event: KeyboardEvent) {
    const step = 60;
    if (event.key === '+' || event.key === '=') zoomCentre(1.3);
    else if (event.key === '-' || event.key === '_') zoomCentre(1 / 1.3);
    else if (event.key === '0') fit();
    else if (event.key === 'ArrowLeft') x += step;
    else if (event.key === 'ArrowRight') x -= step;
    else if (event.key === 'ArrowUp') y += step;
    else if (event.key === 'ArrowDown') y -= step;
    else return;
    event.preventDefault();
  }

  function down(event: PointerEvent) {
    stage.setPointerCapture(event.pointerId);
    pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
    dragging = true;
    if (pointers.size === 2) {
      const [a, b] = [...pointers.values()];
      pinch = { d: Math.hypot(a.x - b.x, a.y - b.y), k };
    }
  }

  function move(event: PointerEvent) {
    const last = pointers.get(event.pointerId);
    if (!last) return;
    const now = { x: event.clientX, y: event.clientY };
    pointers.set(event.pointerId, now);
    if (pointers.size === 1) {
      x += now.x - last.x;
      y += now.y - last.y;
    } else if (pointers.size === 2 && pinch) {
      const [a, b] = [...pointers.values()];
      const rect = stage.getBoundingClientRect();
      zoomAt((pinch.k * (Math.hypot(a.x - b.x, a.y - b.y) / pinch.d)) / k, (a.x + b.x) / 2 - rect.left, (a.y + b.y) / 2 - rect.top);
    }
  }

  function up(event: PointerEvent) {
    pointers.delete(event.pointerId);
    if (pointers.size < 2) pinch = null;
    if (!pointers.size) dragging = false;
  }

  // A click on the backdrop, outside the dialog's box, closes it.
  function backdrop(event: MouseEvent) {
    if (event.target !== dialog) return;
    const r = dialog.getBoundingClientRect();
    if (event.clientX < r.left || event.clientX > r.right || event.clientY < r.top || event.clientY > r.bottom) dialog.close();
  }
</script>

<svelte:window onresize={fit} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<dialog class="zoom" aria-label="Diagram: {caption}" bind:this={dialog} onclose={onclose} onkeydown={onKey} onclick={backdrop}>
  <div class="zoom-bar">
    <div class="zoom-caption">{caption}</div>
    <div class="zoom-tools">
      <span class="zoom-hint">Drag to pan, ⌘ or Ctrl and scroll to zoom</span>
      <button class="round small" type="button" aria-label="Zoom out" title="Zoom out (−)" onclick={() => zoomCentre(1 / 1.3)}><Icon name="zoom-out" /></button>
      <button class="round small" type="button" aria-label="Zoom in" title="Zoom in (+)" onclick={() => zoomCentre(1.3)}><Icon name="zoom" /></button>
      <button class="round small" type="button" aria-label="Fit" title="Fit (0)" onclick={fit}><Icon name="fit" /></button>
      <!-- svelte-ignore a11y_autofocus -->
      <button class="round close" type="button" autofocus aria-label="Close" title="Close (Esc)" onclick={() => dialog.close()}><Icon name="close" size={26} /></button>
    </div>
  </div>
  <div
    class="zoom-stage"
    class:dragging
    bind:this={stage}
    role="presentation"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    ondblclick={(e) => {
      const rect = stage.getBoundingClientRect();
      zoomAt(e.shiftKey ? 1 / 1.6 : 1.6, e.clientX - rect.left, e.clientY - rect.top);
    }}
  >
    <div class="zoom-content" style:transform="translate({x}px, {y}px) scale({k})">{@html svg}</div>
  </div>
</dialog>
