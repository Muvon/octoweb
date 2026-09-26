// Keep normal clicks intact; a 250 ms hold picks up an item. Commit only on
// release inside the list. Pointer capture keeps release/cancel reliable.
function createHoldReorder(container, options) {
  let drag = null;
  let suppressClick = false;
  const marker = document.createElement('div');
  marker.style.cssText = 'position:fixed;pointer-events:none;background:var(--accent);border-radius:2px;z-index:10001;display:none';
  document.body.appendChild(marker);

  function clear() {
    if (!drag) return;
    const old = drag;
    drag = null;
    clearTimeout(old.timer);
    cancelAnimationFrame(old.frame);
    old.row.style.opacity = '';
    if (old.ghost) old.ghost.remove();
    marker.style.display = 'none';
    container.style.cursor = '';
    if (container.hasPointerCapture(old.pointerId)) container.releasePointerCapture(old.pointerId);
  }

  function update() {
    if (!drag || !drag.active) return;
    const d = drag;
    d.ghost.style.left = (d.x - d.offsetX) + 'px';
    d.ghost.style.top = (d.y - d.offsetY) + 'px';
    const bounds = (options.scroller || container).getBoundingClientRect();
    d.target = null;
    marker.style.display = 'none';
    if (!d.moved) return;
    if (d.x < bounds.left || d.x > bounds.right || d.y < bounds.top || d.y > bounds.bottom) return;
    const source = d.row.getBoundingClientRect();
    if (d.x >= source.left && d.x <= source.right && d.y >= source.top && d.y <= source.bottom) return;
    const rows = Array.from(container.querySelectorAll(options.selector))
      .filter(row => row !== d.row && row.getBoundingClientRect().width > 0);
    const position = options.horizontal ? d.x : d.y;
    for (const row of rows) {
      const rect = row.getBoundingClientRect();
      const middle = options.horizontal ? (rect.left + rect.right) / 2 : (rect.top + rect.bottom) / 2;
      d.target = row;
      d.after = position >= middle;
      if (position <= (options.horizontal ? rect.right : rect.bottom)) break;
    }
    if (!d.target) return;
    const rect = d.target.getBoundingClientRect();
    marker.style.display = 'block';
    marker.style.left = (options.horizontal ? (d.after ? rect.right : rect.left) - 1 : rect.left) + 'px';
    marker.style.top = (options.horizontal ? rect.top : (d.after ? rect.bottom : rect.top) - 1) + 'px';
    marker.style.width = (options.horizontal ? 2 : rect.width) + 'px';
    marker.style.height = (options.horizontal ? rect.height : 2) + 'px';
  }

  function scroll() {
    if (!drag || !drag.active) return;
    if (options.scroller) {
      const rect = options.scroller.getBoundingClientRect();
      if (drag.x >= rect.left && drag.x <= rect.right && drag.y >= rect.top && drag.y <= rect.bottom) {
        if (drag.y < rect.top + 28) options.scroller.scrollTop -= 6;
        if (drag.y > rect.bottom - 28) options.scroller.scrollTop += 6;
      }
    }
    update();
    drag.frame = requestAnimationFrame(scroll);
  }

  document.addEventListener('pointerdown', function() { suppressClick = false; }, true);
  container.addEventListener('pointerdown', function(e) {
    if (e.button !== 0 || e.pointerType !== 'mouse' || drag) return;
    const row = e.target.closest(options.selector);
    if (!row || !container.contains(row) || !options.canDrag(row, e.target)) return;
    const rect = row.getBoundingClientRect();
    const d = drag = { row, pointerId: e.pointerId, x: e.clientX, y: e.clientY,
      startX: e.clientX, startY: e.clientY, moved: false,
      offsetX: e.clientX - rect.left, offsetY: e.clientY - rect.top, active: false };
    d.timer = setTimeout(function() {
      if (drag !== d) return;
      d.active = true;
      suppressClick = true;
      container.setPointerCapture(d.pointerId);
      d.ghost = row.cloneNode(true);
      d.ghost.removeAttribute('id');
      d.ghost.setAttribute('aria-hidden', 'true');
      d.ghost.style.cssText = 'position:fixed;pointer-events:none;z-index:10000;margin:0;opacity:0.9;transform:none;background:var(--glass-thick);width:' + rect.width + 'px;height:' + rect.height + 'px';
      document.body.appendChild(d.ghost);
      row.style.opacity = '0.35';
      container.style.cursor = 'grabbing';
      scroll();
    }, 250);
  });
  document.addEventListener('pointermove', function(e) {
    if (!drag || e.pointerId !== drag.pointerId) return;
    if (!(e.buttons & 1)) { clear(); return; }
    drag.x = e.clientX;
    drag.y = e.clientY;
    drag.moved ||= Math.hypot(drag.x - drag.startX, drag.y - drag.startY) > 4;
    if (drag.active) { e.preventDefault(); update(); }
  });
  document.addEventListener('pointerup', function(e) {
    if (!drag || e.pointerId !== drag.pointerId) return;
    drag.x = e.clientX;
    drag.y = e.clientY;
    update();
    const d = drag;
    clear();
    if (d.active && d.target) options.onDrop(d.row, d.target, d.after);
  });
  container.addEventListener('dragstart', function(e) { e.preventDefault(); });
  document.addEventListener('click', function(e) {
    if (!suppressClick || e.detail === 0) return;
    suppressClick = false;
    e.preventDefault();
    e.stopImmediatePropagation();
  }, true);
  document.addEventListener('pointercancel', clear);
  container.addEventListener('lostpointercapture', clear);
  window.addEventListener('blur', clear);
  document.addEventListener('keydown', function(e) {
    if (drag && e.key === 'Escape') {
      e.preventDefault();
      e.stopImmediatePropagation();
      clear();
    }
  }, true);
  return { cancel: clear };
}
