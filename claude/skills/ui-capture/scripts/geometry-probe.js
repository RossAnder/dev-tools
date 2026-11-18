// Browser-side layout check for the ui-capture contract. Load it into the page under capture
// (a script tag, Playwright's addScriptTag, or render.sh's injection), wait for fonts and two
// animation frames, then call uiCaptureGeometry(): it returns the state's geometry.json array,
// [{kind, locator, detail}] with kind overflow, clipping, overlap or offscreen, empty when clean.
// It reads layout only and never changes the page.
(function (global) {
  'use strict';

  // Sub-pixel rounding and font ascent/descent overhang stay under this many CSS pixels.
  var TOLERANCE = 1;
  var OVERLAP_MIN = 2;

  var SKIP_TAGS = {
    HEAD: 1, SCRIPT: 1, STYLE: 1, LINK: 1, META: 1, TITLE: 1, NOSCRIPT: 1, TEMPLATE: 1, BR: 1, WBR: 1
  };
  var REPLACED_TAGS = {
    IMG: 1, SVG: 1, VIDEO: 1, CANVAS: 1, IFRAME: 1, INPUT: 1, SELECT: 1, TEXTAREA: 1, OBJECT: 1, EMBED: 1
  };

  function round(n) {
    return Math.round(n);
  }

  function rectText(r) {
    return '[' + round(r.left) + ',' + round(r.top) + ' ' + round(r.width) + 'x' + round(r.height) + ']';
  }

  function cssEscape(s) {
    return global.CSS && global.CSS.escape ? global.CSS.escape(s) : String(s).replace(/[^\w-]/g, '\\$&');
  }

  function isUniqueId(el) {
    if (!el.id) return false;
    try {
      return el.ownerDocument.querySelectorAll('#' + cssEscape(el.id)).length === 1;
    } catch (e) {
      return false;
    }
  }

  function segment(el) {
    var s = el.tagName.toLowerCase();
    var cls = typeof el.className === 'string' ? el.className.trim().split(/\s+/).filter(Boolean) : [];
    for (var i = 0; i < cls.length && i < 2; i++) s += '.' + cssEscape(cls[i]);
    var parent = el.parentElement;
    if (parent) {
      var index = Array.prototype.indexOf.call(parent.children, el) + 1;
      if (parent.children.length > 1) s += ':nth-child(' + index + ')';
    }
    return s;
  }

  // A CSS selector for el: its id when unique, else up to five path segments from the nearest
  // ancestor with a unique id, or from the body.
  function locator(el) {
    if (el === el.ownerDocument.documentElement) return 'html';
    if (el === el.ownerDocument.body) return 'body';
    if (isUniqueId(el)) return '#' + cssEscape(el.id);
    var parts = [];
    var node = el;
    while (node && node.nodeType === 1 && parts.length < 5) {
      if (node === node.ownerDocument.body) {
        parts.unshift('body');
        break;
      }
      if (node !== el && isUniqueId(node)) {
        parts.unshift('#' + cssEscape(node.id));
        return parts.join(' > ');
      }
      parts.unshift(segment(node));
      node = node.parentElement;
    }
    return parts.join(' > ');
  }

  function rendered(el, style) {
    if (SKIP_TAGS[el.tagName]) return false;
    if (style.display === 'none') return false;
    if (parseFloat(style.opacity) === 0) return false;
    return el.getClientRects().length > 0;
  }

  // Screen-reader-only and similar visually hidden boxes.
  function visuallyHidden(rect, style) {
    if (rect.width <= 1 || rect.height <= 1) return true;
    return style.clip && style.clip !== 'auto' && /rect\(\s*0/.test(style.clip);
  }

  function clipsOrScrolls(value) {
    return value !== 'visible';
  }

  function scrolls(value) {
    return value === 'auto' || value === 'scroll' || value === 'overlay';
  }

  function intersect(a, b) {
    var left = Math.max(a.left, b.left);
    var right = Math.min(a.right, b.right);
    var top = Math.max(a.top, b.top);
    var bottom = Math.min(a.bottom, b.bottom);
    if (right - left <= OVERLAP_MIN || bottom - top <= OVERLAP_MIN) return null;
    return { left: left, top: top, width: right - left, height: bottom - top };
  }

  // The line boxes of el's own text, from its direct text nodes, which can reach past el's box.
  function ownTextRects(el, doc) {
    var rects = [];
    for (var n = el.firstChild; n; n = n.nextSibling) {
      if (n.nodeType !== 3 || !/\S/.test(n.nodeValue)) continue;
      var range = doc.createRange();
      range.selectNodeContents(n);
      var list = range.getClientRects();
      for (var i = 0; i < list.length; i++) {
        if (list[i].width > 0 && list[i].height > 0) rects.push(list[i]);
      }
    }
    return rects;
  }

  function uiCaptureGeometry(root) {
    var doc = (root && root.ownerDocument) || global.document;
    var win = doc.defaultView || global;
    var scope = root || doc.body || doc.documentElement;
    var out = [];
    var viewW = doc.documentElement.clientWidth || win.innerWidth;
    var viewH = doc.documentElement.clientHeight || win.innerHeight;
    var scrollX = win.scrollX || 0;
    var scrollY = win.scrollY || 0;

    var docWidth = doc.documentElement.scrollWidth;
    if (docWidth > viewW + TOLERANCE) {
      out.push({
        kind: 'overflow',
        locator: 'html',
        detail: 'document scrollWidth ' + docWidth + ' > viewport width ' + viewW
      });
    }

    // Per element: whether an ancestor clips or scrolls it, the nearest out-of-flow ancestor
    // (its layer), and whether an ancestor was already reported offscreen.
    var info = new Map();
    var items = [];
    var walker = doc.createTreeWalker(scope, 1);
    var el = scope;
    while (el) {
      var parentInfo = info.get(el.parentElement) || { clipped: false, layer: null, offscreen: false, skip: false };
      var mine = {
        clipped: parentInfo.clipped,
        layer: parentInfo.layer,
        offscreen: parentInfo.offscreen,
        skip: parentInfo.skip
      };
      info.set(el, mine);
      if (mine.skip) {
        el = walker.nextNode();
        continue;
      }
      var style = win.getComputedStyle(el);
      if (style.display === 'contents') {
        el = walker.nextNode();
        continue;
      }
      if (!rendered(el, style)) {
        mine.skip = true;
        el = walker.nextNode();
        continue;
      }
      var rect = el.getBoundingClientRect();
      // visibility is inherited and overridable, so a hidden box is not measured but its descendants are.
      var hidden = style.visibility !== 'visible' || visuallyHidden(rect, style);
      var isRoot = el === doc.documentElement || el === doc.body;
      var outOfFlow = style.position === 'absolute' || style.position === 'fixed';

      if (!isRoot && !hidden && el instanceof win.HTMLElement && (el.clientWidth > 0 || el.clientHeight > 0)) {
        checkOverflow(el, style, out);
      }

      if (!isRoot && !hidden && !mine.clipped && !mine.offscreen) {
        var left = rect.left < -TOLERANCE;
        var right = rect.right > viewW + TOLERANCE;
        var above = rect.top + scrollY < -TOLERANCE;
        var whollyOut = rect.right <= 0 || rect.left >= viewW || rect.bottom + scrollY <= 0;
        // A box parked wholly offscreen by absolute or fixed positioning is a hiding idiom.
        if ((left || right || above) && !(whollyOut && outOfFlow)) {
          out.push({
            kind: 'offscreen',
            locator: locator(el),
            detail: 'box ' + rectText(rect) + ' outside viewport ' + viewW + 'x' + viewH +
              (scrollX || scrollY ? ' scrolled ' + round(scrollX) + ',' + round(scrollY) : '')
          });
          mine.offscreen = true;
        }
      }

      if (!hidden) {
        var layer = outOfFlow ? el : mine.layer;
        var boxes = REPLACED_TAGS[el.tagName] ? [rect] : ownTextRects(el, doc);
        for (var b = 0; b < boxes.length; b++) items.push({ el: el, rect: boxes[b], layer: layer });
      }

      if (outOfFlow) mine.layer = el;
      // The root's overflow applies to the viewport, so it clips nothing the offscreen check skips.
      if (!isRoot && (clipsOrScrolls(style.overflowX) || clipsOrScrolls(style.overflowY))) mine.clipped = true;
      // Overlap inside a replaced element such as an inline svg is its own drawing.
      if (REPLACED_TAGS[el.tagName]) mine.skip = true;
      el = walker.nextNode();
    }

    checkOverlap(items, out);
    return out;
  }

  function checkOverflow(el, style, out) {
    var overX = el.scrollWidth - el.clientWidth;
    var overY = el.scrollHeight - el.clientHeight;
    var axes = [];
    if (overX > TOLERANCE && !scrolls(style.overflowX)) axes.push('x');
    if (overY > TOLERANCE && !scrolls(style.overflowY)) axes.push('y');
    if (!axes.length) return;
    var clipX = clipsOrScrolls(style.overflowX);
    var clipY = clipsOrScrolls(style.overflowY);
    var detail = [];
    var clipped = false;
    var spilled = false;
    for (var i = 0; i < axes.length; i++) {
      if (axes[i] === 'x') {
        // Single-line text cut with an ellipsis shows that it was shortened.
        if (clipX && style.textOverflow === 'ellipsis') continue;
        detail.push('scrollWidth ' + el.scrollWidth + ' > clientWidth ' + el.clientWidth);
        if (clipX) clipped = true;
        else spilled = true;
      } else {
        detail.push('scrollHeight ' + el.scrollHeight + ' > clientHeight ' + el.clientHeight);
        if (clipY) clipped = true;
        else spilled = true;
      }
    }
    if (!detail.length) return;
    var loc = locator(el);
    if (spilled) out.push({ kind: 'overflow', locator: loc, detail: detail.join('; ') });
    if (clipped) {
      out.push({ kind: 'clipping', locator: loc, detail: detail.join('; ') + ' with overflow ' + style.overflowX + ' ' + style.overflowY });
    }
  }

  // Content boxes that cross in one layer. A box in an absolute or fixed layer is compared only
  // with others in that layer, so menus, tooltips and badges over the page are not reported.
  function checkOverlap(items, out) {
    var serials = new Map();
    function serial(el) {
      if (!serials.has(el)) serials.set(el, serials.size);
      return serials.get(el);
    }
    items.sort(function (a, b) {
      return a.rect.left - b.rect.left;
    });
    var reported = new Set();
    for (var i = 0; i < items.length; i++) {
      var a = items[i];
      for (var j = i + 1; j < items.length; j++) {
        var b = items[j];
        if (b.rect.left >= a.rect.right - OVERLAP_MIN) break;
        if (a.el === b.el || a.layer !== b.layer) continue;
        if (a.el.contains(b.el) || b.el.contains(a.el)) continue;
        var hit = intersect(a.rect, b.rect);
        if (!hit) continue;
        var key = serial(a.el) < serial(b.el) ? serial(a.el) + ':' + serial(b.el) : serial(b.el) + ':' + serial(a.el);
        if (reported.has(key)) continue;
        reported.add(key);
        out.push({
          kind: 'overlap',
          locator: locator(a.el) + ' | ' + locator(b.el),
          detail: rectText(a.rect) + ' crosses ' + rectText(b.rect) + ' by ' + round(hit.width) + 'x' + round(hit.height)
        });
      }
    }
  }

  global.uiCaptureGeometry = uiCaptureGeometry;
})(typeof window !== 'undefined' ? window : this);
