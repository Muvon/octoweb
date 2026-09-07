//! Page-to-markdown walker for "copy page as markdown". Installed into the
//! tab on demand; `window.__octowebMarkdown()` returns the main content as
//! markdown with the title and URL on top. Navigation chrome, scripts, and
//! forms are skipped; links come out absolute.

pub const INSTALL: &str = r#"window.__octowebMarkdown = window.__octowebMarkdown || function() {
  var SKIP = {SCRIPT:1,STYLE:1,NOSCRIPT:1,TEMPLATE:1,SVG:1,NAV:1,HEADER:1,FOOTER:1,ASIDE:1,IFRAME:1,BUTTON:1,FORM:1,INPUT:1,SELECT:1,TEXTAREA:1,CANVAS:1};
  var root = document.querySelector('main, article, [role=main]') || document.body;
  function inline(node) {
    var out = '';
    node.childNodes.forEach(function(c) {
      if (c.nodeType === 3) { out += c.nodeValue.replace(/\s+/g, ' '); return; }
      if (c.nodeType !== 1) return;
      var t = c.tagName;
      if (SKIP[t]) return;
      if (t === 'BR') { out += '\n'; return; }
      if (t === 'IMG') { var a = c.getAttribute('alt'); out += a ? '![' + a + '](' + c.src + ')' : ''; return; }
      if (t === 'A' && c.href && !/^javascript:/i.test(c.href)) { var l = inline(c).trim(); out += l ? '[' + l + '](' + c.href + ')' : ''; return; }
      if (t === 'STRONG' || t === 'B') { var b = inline(c).trim(); out += b ? '**' + b + '**' : ''; return; }
      if (t === 'EM' || t === 'I') { var e = inline(c).trim(); out += e ? '*' + e + '*' : ''; return; }
      if (t === 'CODE') { out += '`' + c.textContent + '`'; return; }
      out += inline(c);
    });
    return out;
  }
  var lines = [];
  function list(node, ordered, depth) {
    var i = 0;
    node.childNodes.forEach(function(li) {
      if (li.nodeType !== 1 || li.tagName !== 'LI') return;
      i++;
      var text = '', nested = [];
      li.childNodes.forEach(function(x) {
        if (x.nodeType === 1 && (x.tagName === 'UL' || x.tagName === 'OL')) nested.push(x);
        else if (x.nodeType === 3) text += x.nodeValue;
        else if (x.nodeType === 1 && !SKIP[x.tagName]) text += inline(x);
      });
      lines.push('  '.repeat(depth) + (ordered ? i + '. ' : '- ') + text.replace(/\s+/g, ' ').trim());
      nested.forEach(function(n) { list(n, n.tagName === 'OL', depth + 1); });
    });
  }
  function block(node) {
    node.childNodes.forEach(function(c) {
      if (c.nodeType === 3) { var s = c.nodeValue.trim(); if (s) lines.push(s, ''); return; }
      if (c.nodeType !== 1) return;
      var t = c.tagName;
      if (SKIP[t]) return;
      if (/^H[1-6]$/.test(t)) { lines.push('#'.repeat(+t[1]) + ' ' + inline(c).trim(), ''); return; }
      if (t === 'P') { var p = inline(c).trim(); if (p) lines.push(p, ''); return; }
      if (t === 'PRE') { lines.push('```', c.textContent.replace(/\n$/, ''), '```', ''); return; }
      if (t === 'BLOCKQUOTE') { var q = inline(c).trim(); if (q) lines.push(q.split('\n').map(function(l) { return '> ' + l; }).join('\n'), ''); return; }
      if (t === 'HR') { lines.push('---', ''); return; }
      if (t === 'UL' || t === 'OL') { list(c, t === 'OL', 0); lines.push(''); return; }
      if (t === 'TABLE') {
        var rows = [];
        c.querySelectorAll('tr').forEach(function(tr) {
          var cells = [];
          tr.querySelectorAll('th,td').forEach(function(td) { cells.push(inline(td).replace(/\s+/g, ' ').trim().replace(/\|/g, '\\|')); });
          if (cells.length) rows.push('| ' + cells.join(' | ') + ' |');
        });
        if (rows.length) {
          lines.push(rows[0], '|' + rows[0].split('|').slice(1, -1).map(function() { return ' --- '; }).join('|') + '|');
          rows.slice(1).forEach(function(r) { lines.push(r); });
          lines.push('');
        }
        return;
      }
      block(c);
    });
  }
  block(root);
  var body = lines.join('\n').replace(/\n{3,}/g, '\n\n').trim();
  return '# ' + (document.title || location.href) + '\n\n<' + location.href + '>\n\n' + body + '\n';
};"#;
