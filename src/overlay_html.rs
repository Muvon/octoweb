/// Returns the full HTML for the CMD+K overlay window.
/// The page is injected with `window.__items` (JSON array) before being shown.
/// Each item: { title, url, kind } where kind = "tab" | "history"
///
/// Tahoe liquid glass design — light/dark adaptive via prefers-color-scheme.
pub fn html() -> &'static str {
    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; }

  /* ── Tahoe Liquid Glass tokens ─────────────────────────────────────────── */
  :root {
    /* Glass panel — light: frosted white */
    --glass-bg:        rgba(255, 255, 255, 0.72);
    --glass-border:    rgba(0, 0, 0, 0.08);
    --glass-inner:     rgba(255, 255, 255, 0.50);
    --glass-shadow:    0 24px 80px rgba(0, 0, 0, 0.18), 0 2px 8px rgba(0, 0, 0, 0.08);

    /* Input */
    --input-bg:        rgba(255, 255, 255, 0.85);
    --input-border:    rgba(0, 0, 0, 0.06);
    --input-focus:     rgba(0, 122, 255, 0.25);

    /* Text */
    --text-primary:    rgba(0, 0, 0, 0.90);
    --text-secondary:  rgba(0, 0, 0, 0.55);
    --text-tertiary:   rgba(0, 0, 0, 0.30);

    /* Items */
    --item-hover:      rgba(0, 122, 255, 0.08);
    --item-selected:   rgba(0, 122, 255, 0.14);

    /* Accent */
    --accent:          #007aff;
    --accent-hover:    #0066d6;

    /* Section headers */
    --section-text:    rgba(0, 0, 0, 0.40);
    --section-border:  rgba(0, 0, 0, 0.06);

    /* Scrollbar */
    --scrollbar:       rgba(0, 0, 0, 0.12);
  }

  @media (prefers-color-scheme: dark) {
    :root {
      --glass-bg:        rgba(28, 28, 32, 0.82);
      --glass-border:    rgba(255, 255, 255, 0.08);
      --glass-inner:     rgba(255, 255, 255, 0.04);
      --glass-shadow:    0 24px 80px rgba(0, 0, 0, 0.55), 0 2px 8px rgba(0, 0, 0, 0.30);

      --input-bg:        rgba(255, 255, 255, 0.08);
      --input-border:    rgba(255, 255, 255, 0.10);
      --input-focus:     rgba(10, 132, 255, 0.30);

      --text-primary:    rgba(255, 255, 255, 0.92);
      --text-secondary:  rgba(255, 255, 255, 0.55);
      --text-tertiary:   rgba(255, 255, 255, 0.30);

      --item-hover:      rgba(10, 132, 255, 0.10);
      --item-selected:   rgba(10, 132, 255, 0.18);

      --accent:          #0a84ff;
      --accent-hover:    #409cff;

      --section-text:    rgba(255, 255, 255, 0.42);
      --section-border:  rgba(255, 255, 255, 0.06);

      --scrollbar:       rgba(255, 255, 255, 0.10);
    }
  }

  html, body {
    width: 100%;
    height: 100%;
    background: transparent;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
    -webkit-font-smoothing: antialiased;
    color: var(--text-primary);
  }

  #backdrop {
    position: fixed;
    inset: 0;
    background: radial-gradient(ellipse at top, rgba(0, 0, 0, 0.12), rgba(0, 0, 0, 0.28));
    backdrop-filter: blur(12px) saturate(180%);
    -webkit-backdrop-filter: blur(12px) saturate(180%);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 12vh;
  }

  #modal {
    width: min(680px, calc(100vw - 32px));
    background: var(--glass-bg);
    backdrop-filter: blur(48px) saturate(200%);
    -webkit-backdrop-filter: blur(48px) saturate(200%);
    border: 1px solid var(--glass-border);
    border-radius: 14px;
    box-shadow: var(--glass-shadow), inset 0 1px 0 var(--glass-inner);
    overflow: hidden;
    transform: translateY(-12px) scale(0.97);
    opacity: 0;
    animation: reveal 180ms cubic-bezier(0.16, 1, 0.3, 1) forwards;
  }

  @keyframes reveal {
    to {
      transform: translateY(0) scale(1);
      opacity: 1;
    }
  }

  #search-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--section-border);
  }

  #search-icon {
    width: 18px;
    height: 18px;
    color: var(--text-tertiary);
    flex-shrink: 0;
  }

  #query {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 16px;
    font-weight: 400;
    letter-spacing: -0.01em;
    caret-color: var(--accent);
  }

  #query::placeholder {
    color: var(--text-tertiary);
  }

  #action-badge {
    padding: 5px 11px;
    border-radius: 999px;
    border: 1px solid var(--input-border);
    background: var(--input-bg);
    font-size: 11px;
    font-weight: 500;
    color: var(--text-secondary);
    letter-spacing: 0.01em;
    white-space: nowrap;
  }

  #results {
    max-height: min(420px, 56vh);
    overflow-y: auto;
    padding: 6px;
  }

  #results::-webkit-scrollbar {
    width: 6px;
  }

  #results::-webkit-scrollbar-track {
    background: transparent;
  }

  #results::-webkit-scrollbar-thumb {
    background: var(--scrollbar);
    border-radius: 3px;
  }

  .section-header {
    padding: 8px 10px 4px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--section-text);
  }

  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 8px;
    cursor: default;
    transition: background 80ms ease;
  }

  .item.selected,
  .item:hover {
    background: var(--item-selected);
  }

  .item-icon {
    width: 18px;
    height: 18px;
    color: var(--text-secondary);
    flex-shrink: 0;
  }

  .item-favicon {
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    border-radius: 4px;
    object-fit: contain;
    background: var(--glass-bg);
  }

  .item-text {
    flex: 1;
    min-width: 0;
  }

  .item-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .item-title .match {
    color: var(--accent);
    font-weight: 600;
  }

  .item-url {
    margin-top: 1px;
    font-size: 11px;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    color: var(--text-tertiary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .item-meta {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .kind-pill {
    font-size: 10px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-secondary);
    border: 1px solid var(--input-border);
    border-radius: 999px;
    padding: 2px 7px;
  }

  .close-btn {
    width: 18px;
    height: 18px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--text-tertiary);
    font-size: 14px;
    line-height: 18px;
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.1s ease, background 0.1s ease, color 0.1s ease;
  }

  .item:hover .close-btn,
  .item.selected .close-btn {
    opacity: 1;
  }

  .close-btn:hover {
    background: rgba(255, 59, 48, 0.12);
    color: #ff3b30;
  }

  .shortcut-badge {
    font-size: 10px;
    font-weight: 500;
    font-family: inherit;
    color: var(--text-tertiary);
    border: 1px solid var(--input-border);
    border-radius: 4px;
    padding: 1px 5px;
    letter-spacing: 0.02em;
  }

  #hint {
    border-top: 1px solid var(--section-border);
    padding: 10px 14px;
    text-align: center;
    font-size: 11px;
    color: var(--text-tertiary);
    letter-spacing: 0.01em;
  }

  #hint kbd {
    display: inline-block;
    padding: 2px 5px;
    margin: 0 2px;
    font-family: inherit;
    font-size: 10px;
    background: var(--input-bg);
    border: 1px solid var(--input-border);
    border-radius: 4px;
  }
</style>
</head>
<body>
<div id="backdrop">
  <div id="modal">
    <div id="search-row">
      <svg id="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="11" cy="11" r="7"></circle><line x1="21" y1="21" x2="16.65" y2="16.65"></line>
      </svg>
      <input id="query" type="text" autocomplete="off" spellcheck="false" placeholder="Search tabs, history, or enter URL" />
      <div id="action-badge">↵ Open</div>
    </div>
    <div id="results"></div>
    <div id="hint"><kbd>↑↓</kbd> navigate · <kbd>⌘1</kbd>–<kbd>⌘9</kbd> jump · <kbd>↵</kbd> confirm · <kbd>⌘↵</kbd> open/search · <kbd>⌘⇧↵</kbd> ask AI · <kbd>Esc</kbd> close · <kbd>⌘W</kbd> close tab</div>
  </div>
</div>

<script>
// https://github.com/farzher/fuzzysort v3.0.2 — MIT License
((r,e)=>{"function"==typeof define&&define.amd?define([],e):"object"==typeof module&&module.exports?module.exports=e():r.fuzzysort=e()})(this,c=>{var f=r=>{"number"==typeof r?r=""+r:"string"!=typeof r&&(r="");var e=u(r);return x(r,{t:e.i,o:e.v,u:e.l})};class M{get["indexes"](){return this.p.slice(0,this.p.g).sort((r,e)=>r-e)}set["indexes"](r){return this.p=r}["highlight"](r,e){return((r,e="<b>",f="</b>")=>{for(var t="function"==typeof e?e:void 0,i=r.target,a=i.length,o=r.indexes,n="",v=0,u=0,s=!1,l=[],c=0;c<a;++c){var p=i[c];if(o[u]===c){if(++u,s||(s=!0,t?(l.push(n),n=""):n+=e),u===o.length){t?(l.push(t(n+=p,v++)),n="",l.push(i.substr(c+1))):n+=p+f+i.substr(c+1);break}}else s&&(s=!1,t?(l.push(t(n,v++)),n=""):n+=f);n+=p}return t?l:n})(this,r,e)}get["score"](){return e(this.h)}set["score"](r){this.h=A(r)}}class j extends Array{get["score"](){return e(this.h)}set["score"](r){this.h=A(r)}}var o,n,r,t,x=(r,e)=>{var f=new M;return f.target=r,f.obj=e.obj??Q,f.h=e.h??O,f.p=e.p??[],f.t=e.t??"",f.o=e.o??Q,f.k=e.k??Q,f.u=e.u??0,f},e=r=>r===O?0:1<r?r:Math.E**(-2*((1-r)**.04307-1)),A=r=>0===r?O:1<r?r:1-Math.pow(Math.log(r)/-2+1,1/.04307),i=r=>{"number"==typeof r?r=""+r:"string"!=typeof r&&(r=""),r=r.trim();var e=u(r),f=[];if(e.S)for(var t,i=r.split(/\s+/),i=[...new Set(i)],a=0;a<i.length;a++)""!==i[a]&&(t=u(i[a]),f.push({v:t.v,i:i[a].toLowerCase(),S:!1}));return{v:e.v,i:e.i,S:e.S,l:e.l,_:f}},L=r=>{var e;return 999<r.length?f(r):(void 0===(e=a.get(r))&&(e=f(r),a.set(r,e)),e)},D=r=>{var e;return 999<r.length?i(r):(void 0===(e=l.get(r))&&(e=i(r),l.set(r,e)),e)},F=(r,e,f=!1,t=!1)=>{if(!1===f&&r.S)return C(r,e,t);for(var f=r.i,i=r.v,a=i[0],o=e.o,n=i.length,v=o.length,u=0,s=0,l=0;;){if(a===o[s]){if(q[l++]=s,++u===n)break;a=i[u]}if(v<=++s)return Q}var u=0,c=!1,p=0,b=e.k,d=(b===Q&&(b=e.k=N(e.target)),0);if((s=0===q[0]?0:b[q[0]-1])!==v)for(;;)if(v<=s){if(u<=0)break;if(200<++d)break;--u;var w=z[--p],s=b[w]}else if(i[u]===o[s]){if(z[p++]=s,++u===n){c=!0;break}++s}else s=b[s];var g=n<=1?-1:e.t.indexOf(f,q[0]),h=!!~g,y=h&&(0===g||e.k[g-1]===g);if(h&&!y)for(var k=0;k<b.length;k=b[k])if(!(k<=g)){for(var S=0;S<n&&i[S]===e.o[k+S];S++);if(S===n){g=k,y=!0;break}}t=r=>{for(var e=0,f=0,t=1;t<n;++t)r[t]-r[t-1]!=1&&(e-=r[t],++f);if(e-=(12+(r[n-1]-r[0]-(n-1)))*f,0!==r[0]&&(e-=r[0]*r[0]*.2),c){for(var i=1,t=b[0];t<v;t=b[t])++i;24<i&&(e*=10*(i-24))}else e*=1e3;return e-=(v-n)/2,h&&(e/=1+n*n*1),y&&(e/=1+n*n*1),e-=(v-n)/2};if(c)if(y){for(k=0;k<n;++k)q[k]=g+k;_=q,m=t(q)}else _=z,m=t(z);else{if(h)for(var k=0;k<n;++k)q[k]=g+k;var _,m=t(_=q)}e.h=m;for(k=0;k<n;++k)e.p[k]=_[k];e.p.g=n;r=new M;return r.target=e.target,r.h=e.h,r.p=e.p,r},C=(r,e,f)=>{for(var t=new Set,i=0,a=Q,o=0,n=r._,v=n.length,u=0,s=()=>{for(let r=u-1;0<=r;r--)e.k[S[2*r+0]]=S[2*r+1]},l=!1,c=0;c<v;++c){E[c]=O;var p=n[c],a=F(p,e);if(f){if(a===Q)continue;l=!0}else if(a===Q)return s(),Q;if(!(c===v-1)){var b=a.p,d=!0;for(let r=0;r<b.g-1;r++)if(b[r+1]-b[r]!=1){d=!1;break}if(d){var w=b[b.g-1]+1,g=e.k[w-1];for(let r=w-1;0<=r&&g===e.k[r];r--)e.k[r]=w,S[2*u+0]=r,S[2*u+1]=g,u++}}i+=a.h/v,E[c]=a.h/v,a.p[0]<o&&(i-=2*(o-a.p[0]));for(var o=a.p[0],h=0;h<a.p.g;++h)t.add(a.p[h])}if(f&&!l)return Q;s();var y=F(r,e,!0);if(y!==Q&&y.h>i){if(f)for(c=0;c<v;++c)E[c]=y.h/v;return y}(a=f?e:a).h=i;var k,c=0;for(k of t)a.p[c++]=k;return a.p.g=c,a},v=r=>r.replace(/\p{Script=Latin}+/gu,r=>r.normalize("NFD")).replace(/[\u0300-\u036f]/g,""),u=r=>{for(var e=(r=v(r)).length,f=r.toLowerCase(),t=[],i=0,a=!1,o=0;o<e;++o){var n=t[o]=f.charCodeAt(o);32===n?a=!0:i|=1<<(97<=n&&n<=122?n-97:48<=n&&n<=57?26:n<=127?30:31)}return{v:t,l:i,S:a,i:f}},s=r=>{for(var e=r.length,f=[],t=0,i=!1,a=!1,o=0;o<e;++o){var n=r.charCodeAt(o),v=65<=n&&n<=90,n=v||97<=n&&n<=122||48<=n&&n<=57,u=v&&!i||!a||!n,i=v,a=n;u&&(f[t++]=o)}return f},N=r=>{for(var e=(r=v(r)).length,f=s(r),t=[],i=f[0],a=0,o=0;o<e;++o)o<i?t[o]=i:(i=f[++a],t[o]=void 0===i?e:i);return t},a=new Map,l=new Map,q=[],z=[],S=[],B=[],E=[],G=[],H=[],I=(r,e)=>{var f=r[e];if(void 0!==f)return f;if("function"==typeof e)return e(r);for(var t=e,i=(t=Array.isArray(e)?t:e.split(".")).length,a=-1;r&&++a<i;)r=r[t[a]];return r},J=r=>"object"==typeof r&&"number"==typeof r.u,K=1/0,O=-K,P=[],Q=(P.total=0,null),R=f(""),T=(o=[],n=0,t=r=>{for(var e=o[i=0],f=1;f<n;){var t=f+1,i=f;t<n&&o[t].h<o[f].h&&(i=t),o[i-1>>1]=o[i],f=1+(i<<1)}for(var a=i-1>>1;0<i&&e.h<o[a].h;a=(i=a)-1>>1)o[i]=o[a];o[i]=e},(r={}).add=r=>{var e=n;o[n++]=r;for(var f=e-1>>1;0<e&&r.h<o[f].h;f=(e=f)-1>>1)o[e]=o[f];o[e]=r},r.m=r=>{var e;if(0!==n)return e=o[0],o[0]=o[--n],t(),e},r.M=r=>{if(0!==n)return o[0]},r.C=r=>{o[0]=r,t()},r);return{single:(r,e)=>{var f;return!r||!e||(r=D(r),J(e)||(e=L(e)),((f=r.l)&e.u)!==f)?Q:F(r,e)},go:(r,e,f)=>{if(!r)return f?.all?((r,e)=>{var f=[],t=(f.total=r.length,e?.limit||K);if(e?.key)for(var i=0;i<r.length;i++){var a=r[i];var o=I(a,e.key);if(o==Q)continue;if(!J(o))o=L(o);var n=x(o.target,{h:o.h,obj:a});f.push(n);if(f.length>=t)return f}else if(e?.keys)for(var i=0;i<r.length;i++){var a=r[i];var v=new j(e.keys.length);for(var u=e.keys.length-1;u>=0;--u){var o=I(a,e.keys[u]);if(!o){v[u]=R;continue}if(!J(o))o=L(o);o.h=O;o.p.g=0;v[u]=o}v.obj=a;v.h=O;f.push(v);if(f.length>=t)return f}else for(var i=0;i<r.length;i++){var o=r[i];if(o==Q)continue;if(!J(o))o=L(o);o.h=O;o.p.g=0;f.push(o);if(f.length>=t)return f}return f})(e,f):P;var t=D(r),i=t.l,a=t.S,o=A(f?.threshold||0),n=f?.limit||K,v=0,u=0,s=e.length;function l(r){v<n?(T.add(r),++v):(++u,r.h>T.M().h&&T.C(r))}if(f?.key)for(var c=f.key,p=0;p<s;++p){var b=e[p];!(m=I(b,c))||(i&(m=J(m)?m:L(m)).u)!==i||(M=F(t,m))===Q||M.h<o||(M.obj=b,l(M))}else if(f?.keys){var d=f.keys,w=d.length;r:for(p=0;p<s;++p){for(var b=e[p],g=0,h=0;h<w;++h){c=d[h];(m=I(b,c))?(J(m)||(m=L(m)),g|=(G[h]=m).u):G[h]=R}if((i&g)===i){if(a)for(let r=0;r<t._.length;r++)B[r]=O;for(h=0;h<w;++h)if((m=G[h])===R)H[h]=R;else if(H[h]=F(t,m,!1,a),H[h]===Q)H[h]=R;else if(a)for(let r=0;r<t._.length;r++)-1e3<E[r]&&B[r]>O&&(_=(B[r]+E[r])/4)>B[r]&&(B[r]=_),E[r]>B[r]&&(B[r]=E[r]);if(a){for(let r=0;r<t._.length;r++)if(B[r]===O)continue r}else{var y=!1;for(let r=0;r<w;r++)if(H[r].h!==O){y=!0;break}if(!y)continue}var k=new j(w);for(let r=0;r<w;r++)k[r]=H[r];if(a){var S=0;for(let r=0;r<t._.length;r++)S+=B[r]}else{var _,S=O;for(let r=0;r<w;r++)(S=-1e3<(M=k[r]).h&&O<S&&S<(_=(S+M.h)/4)?_:S)<M.h&&(S=M.h)}if(k.obj=b,k.h=S,f?.scoreFn){if(!(S=f.scoreFn(k)))continue;S=A(S),k.h=S}S<o||l(k)}}}else for(var m,M,p=0;p<s;++p)!(m=e[p])||(i&(m=J(m)?m:L(m)).u)!==i||(M=F(t,m))===Q||M.h<o||l(M);if(0===v)return P;for(var C=new Array(v),p=v-1;0<=p;--p)C[p]=T.m();return C.total=v+u,C},prepare:f,cleanup:()=>{a.clear(),l.clear()}}});
</script>
<script>
(function() {
  const queryEl = document.getElementById('query');
  const resultsEl = document.getElementById('results');
  const actionBadge = document.getElementById('action-badge');

  let items = [];
  let filtered = [];
  let sel = 0;

  const ICONS = {
    search: '<svg class="item-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"></circle><line x1="21" y1="21" x2="16.65" y2="16.65"></line></svg>',
    globe: '<svg class="item-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"></circle><line x1="2" y1="12" x2="22" y2="12"></line><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"></path></svg>',
    tab: '<svg class="item-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"></rect><line x1="3" y1="9" x2="21" y2="9"></line></svg>',
    history: '<svg class="item-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 3v5h5"></path><path d="M3.05 13A9 9 0 1 0 6 6.3L3 8"></path><line x1="12" y1="7" x2="12" y2="12"></line><line x1="12" y1="12" x2="15" y2="15"></line></svg>',
    ai: '<svg class="item-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2l2.09 6.26L20 10l-5.91 1.74L12 18l-2.09-6.26L4 10l5.91-1.74L12 2z"></path><path d="M18 14l1.18 3.54L22.72 19l-3.54 1.18L18 24l-1.18-3.54L13.28 19l3.54-1.18L18 14z"></path></svg>'
  };

  window.__setItems = function(data) {
    items = Array.isArray(data) ? data : [];
    queryEl.value = '';
    sel = 0;
    window.ipc.postMessage(JSON.stringify({ type: 'overlay_open' }));
    render('');
    queryEl.focus();
  };

  // Refresh items in-place (after tab close / history remove) — keeps current query
  window.__refreshItems = function(data) {
    items = Array.isArray(data) ? data : [];
    sel = Math.min(sel, Math.max(filtered.length - 2, 0));
    render(queryEl.value);
  };

  queryEl.addEventListener('input', () => render(queryEl.value));
  queryEl.addEventListener('keydown', onInputKeyDown);

  document.getElementById('backdrop').addEventListener('mousedown', e => {
    if (e.target === e.currentTarget) closeOverlay();
  });

  function onInputKeyDown(e) {
    if (handleEditingHotkeys(e)) return;

    if (e.key === 'ArrowDown') {
      e.preventDefault();
      move(1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      move(-1);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (e.metaKey && e.shiftKey) {
        // ⌘⇧Enter → Ask AI
        askAI();
      } else if (e.metaKey) {
        // ⌘Enter → force navigate (URL → open, else → search)
        forceNavigate();
      } else {
        confirmSelection();
      }
    } else if (e.key === 'Escape') {
      e.preventDefault();
      closeOverlay();
    } else if (e.key === 'Home') {
      e.preventDefault();
      setCursor(0);
    } else if (e.key === 'End') {
      e.preventDefault();
      setCursor(queryEl.value.length);
    }
  }

  function handleEditingHotkeys(e) {
    const isMacCmd = e.metaKey && !e.ctrlKey && !e.altKey;
    const isCtrl = e.ctrlKey && !e.metaKey && !e.altKey;

    // ⌘1-⌘9, ⌘0 → jump to item 1-9, 10 (only for tabs/history, not actions)
    if (isMacCmd && /^[0-9]$/.test(e.key)) {
      e.preventDefault();
      var idx = e.key === '0' ? 9 : parseInt(e.key, 10) - 1;
      if (idx < filtered.length) {
        var target = filtered[idx];
        if (target.kind === 'tab' || target.kind === 'history') {
          sel = idx;
          confirmSelection();
        }
      }
      return true;
    }

    if (isMacCmd && e.key.toLowerCase() === 'w') {
      e.preventDefault();
      if (filtered.length > 0 && sel >= 0 && sel < filtered.length) {
        const item = filtered[sel];
        if (item.kind === 'tab') {
          window.ipc.postMessage(JSON.stringify({ type: 'close_tab', tab_id: item.tab_id }));
        } else if (item.kind === 'history') {
          window.ipc.postMessage(JSON.stringify({ type: 'remove_history', url: item.url }));
        }
      }
      return true;
    }

    if (isMacCmd && e.key.toLowerCase() === 'v') {
      navigator.clipboard.readText().then(text => {
        if (!text) return;
        const start = queryEl.selectionStart;
        const end = queryEl.selectionEnd;
        const before = queryEl.value.slice(0, start);
        const after = queryEl.value.slice(end);
        queryEl.value = before + text + after;
        const pos = start + text.length;
        queryEl.setSelectionRange(pos, pos);
        render(queryEl.value);
      }).catch(() => {});
      return true;
    }

    if (!isCtrl) return false;

    const key = e.key.toLowerCase();
    if (key === 'a') {
      e.preventDefault();
      setCursor(0);
      return true;
    }
    if (key === 'e') {
      e.preventDefault();
      setCursor(queryEl.value.length);
      return true;
    }
    if (key === 'k') {
      e.preventDefault();
      const start = queryEl.selectionStart;
      queryEl.value = queryEl.value.slice(0, start);
      setCursor(start);
      render(queryEl.value);
      return true;
    }
    if (key === 'u') {
      e.preventDefault();
      const end = queryEl.selectionEnd;
      queryEl.value = queryEl.value.slice(end);
      setCursor(0);
      render(queryEl.value);
      return true;
    }

    if (key === 'p') {
      e.preventDefault();
      move(-1);
      return true;
    }
    if (key === 'n') {
      e.preventDefault();
      move(1);
      return true;
    }

    return false;
  }

  function setCursor(pos) {
    queryEl.setSelectionRange(pos, pos);
  }

  function closeOverlay() {
    window.ipc.postMessage(JSON.stringify({ type: 'overlay_close' }));
    window.ipc.postMessage(JSON.stringify({ type: 'close' }));
  }

  function confirmSelection() {
    if (filtered.length === 0) {
      const q = queryEl.value.trim();
      if (!q) {
        closeOverlay();
        return;
      }
      if (isLikelyUrl(q)) {
        navigate(toNavigableUrl(q));
      } else {
        navigate(searchUrl(q));
      }
      return;
    }

    const item = filtered[sel];
    if (item.kind === 'tab') {
      window.ipc.postMessage(JSON.stringify({ type: 'switch_tab', tab_id: item.tab_id }));
      return;
    }

    if (item.kind === 'search') {
      navigate(searchUrl(item.query));
      return;
    }

    if (item.kind === 'ask') {
      askAI();
      return;
    }

    navigate(item.url);
  }

  function forceNavigate() {
    const q = queryEl.value.trim();
    if (!q) return;
    if (isLikelyUrl(q)) {
      navigate(toNavigableUrl(q));
    } else {
      navigate(searchUrl(q));
    }
  }

  function askAI() {
    const q = queryEl.value.trim();
    if (!q) return;
    window.ipc.postMessage(JSON.stringify({ type: 'ask_ai', text: q }));
  }

  function navigate(url) {
    window.ipc.postMessage(JSON.stringify({ type: 'navigate', url }));
  }

  function searchUrl(q) {
    return 'https://www.google.com/search?q=' + encodeURIComponent(q);
  }

  function toNavigableUrl(raw) {
    const q = raw.trim();
    return hasScheme(q) ? q : 'https://' + q;
  }

  function hasScheme(value) {
    return /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(value);
  }

  function isLikelyUrl(value) {
    const s = value.trim();
    if (!s || /\s/.test(s)) return false;
    if (hasScheme(s)) return true;

    const host = s.split(/[/?#]/)[0];
    if (host === 'localhost' || /^localhost:\d+$/.test(host)) return true;
    if (/^\d{1,3}(\.\d{1,3}){3}(:\d+)?$/.test(host)) return true;
    if (/^[a-z0-9-]+(\.[a-z0-9-]+)+(:\d+)?$/i.test(host)) return true;

    return false;
  }

  function move(dir) {
    if (filtered.length === 0) return;
    sel = (sel + dir + filtered.length) % filtered.length;
    renderItems();
    updateBadge();
  }

  function render(rawQuery) {
    const raw = rawQuery.trim();
    const q = raw.toLowerCase();

    if (!raw) {
      filtered = items
        .filter(i => i.kind === 'tab')
        .slice()
        .sort((a, b) => (b.visit_count || 0) - (a.visit_count || 0))
        .slice(0, 12);
      sel = 0;
      renderItems();
      updateBadge();
      return;
    }

    const list = fuzzyFilter(q, items);
    const urlLike = isLikelyUrl(raw);
    const openAction = {
      kind: 'url',
      title: 'Open URL',
      url: toNavigableUrl(raw),
      subtitle: toNavigableUrl(raw),
      pill: 'URL'
    };
    const searchAction = {
      kind: 'search',
      title: 'Search Google',
      url: raw,
      query: raw,
      subtitle: raw,
      pill: 'Search'
    };
    const askAction = {
      kind: 'ask',
      title: 'Ask AI',
      url: '',
      query: raw,
      subtitle: raw,
      pill: 'AI'
    };

    const actions = urlLike ? [openAction, searchAction, askAction] : [searchAction, openAction, askAction];
    filtered = [...list, ...actions].slice(0, 14);
    sel = 0;
    renderItems();
    updateBadge();
  }

  function updateBadge() {
    if (filtered.length === 0) {
      actionBadge.textContent = '↵ Search';
      return;
    }
    const item = filtered[sel];
    if (item.kind === 'tab')          actionBadge.textContent = '↵ Switch';
    else if (item.kind === 'history') actionBadge.textContent = '↵ Open';
    else if (item.kind === 'url')     actionBadge.textContent = '↵ Open URL';
    else if (item.kind === 'ask')     actionBadge.textContent = '↵ Ask AI';
    else                              actionBadge.textContent = '↵ Search';
  }

  function renderItems() {
    resultsEl.innerHTML = '';

    if (filtered.length === 0) {
      resultsEl.innerHTML = '<div style="padding:24px;color:var(--text-tertiary);font-size:13px;text-align:center;">No matches found</div>';
      return;
    }

    // Group by kind
    const tabs = filtered.filter(i => i.kind === 'tab');
    const history = filtered.filter(i => i.kind === 'history');
    const actions = filtered.filter(i => i.kind !== 'tab' && i.kind !== 'history');

    let html = '';

    if (tabs.length > 0) {
      html += '<div class="section-header">Open Tabs</div>';
      tabs.forEach((item, i) => {
        const globalIdx = filtered.indexOf(item);
        html += renderItem(item, globalIdx);
      });
    }

    if (history.length > 0) {
      html += '<div class="section-header">History</div>';
      history.forEach((item, i) => {
        const globalIdx = filtered.indexOf(item);
        html += renderItem(item, globalIdx);
      });
    }

    if (actions.length > 0) {
      html += '<div class="section-header">Actions</div>';
      actions.forEach((item, i) => {
        const globalIdx = filtered.indexOf(item);
        html += renderItem(item, globalIdx);
      });
    }

    resultsEl.innerHTML = html;

    // Attach event listeners
    resultsEl.querySelectorAll('.item').forEach(row => {
      const idx = parseInt(row.dataset.idx, 10);
      row.addEventListener('mousedown', e => {
        if (e.target && e.target.classList && e.target.classList.contains('close-btn')) return;
        e.preventDefault();
        sel = idx;
        confirmSelection();
      });
    });

    resultsEl.querySelectorAll('.close-btn').forEach(btn => {
      btn.addEventListener('mousedown', e => {
        e.preventDefault();
        e.stopPropagation();
        const tabId = Number(btn.getAttribute('data-tab-id'));
        if (Number.isFinite(tabId) && tabId > 0) {
          window.ipc.postMessage(JSON.stringify({ type: 'close_tab', tab_id: tabId }));
          return;
        }
        const histUrl = btn.getAttribute('data-history-url');
        if (histUrl) {
          window.ipc.postMessage(JSON.stringify({ type: 'remove_history', url: histUrl }));
        }
      });
    });
  }

  function renderItem(item, idx) {
    const hostname = cleanHost(item.url || '');
    const rawTitle = (item.title && item.title !== item.url) ? item.title : hostname;
    const kindLabel = esc(item.pill || kindLabelFor(item));
    const selected = idx === sel ? ' selected' : '';
    const isJumpable = item.kind === 'tab' || item.kind === 'history';
    const shortcutNum = isJumpable ? (idx < 9 ? String(idx + 1) : idx === 9 ? '0' : '') : '';
    const actionShortcut = item.kind === 'ask' ? '⌘⇧↵' : (item.kind === 'search' || item.kind === 'url') ? '⌘↵' : '';
    const shortcutHtml = shortcutNum ? '<span class="shortcut-badge">⌘' + shortcutNum + '</span>'
                       : actionShortcut ? '<span class="shortcut-badge">' + actionShortcut + '</span>'
                       : '';
    const canClose = isJumpable;
    const closeAttr = item.kind === 'tab'
      ? 'data-tab-id="' + item.tab_id + '"'
      : 'data-history-url="' + esc(item.url) + '"';
    const closeHtml = canClose
      ? '<button class="close-btn" ' + closeAttr + ' title="Remove">×</button>'
      : '';

    return '<div class="item' + selected + '" data-idx="' + idx + '">' +
      iconHtml(item) +
      '<div class="item-text">' +
        '<div class="item-title">' + esc(rawTitle) + '</div>' +
        '<div class="item-url">' + esc(hostname) + '</div>' +
      '</div>' +
      '<div class="item-meta">' +
        shortcutHtml +
        '<span class="kind-pill">' + kindLabel + '</span>' +
        closeHtml +
      '</div>' +
    '</div>';
  }

  function iconHtml(item) {
    if (item.favicon && (item.kind === 'tab' || item.kind === 'history')) {
      const fallback = item.kind === 'tab' ? ICONS.tab : ICONS.history;
      return '<img class="item-favicon" src="' + esc(item.favicon) + '" onerror="this.outerHTML=\'' + fallback.replace(/"/g, "'") + '\'" />';
    }
    const html = item.kind === 'tab' ? ICONS.tab
               : item.kind === 'history' ? ICONS.history
               : item.kind === 'url' ? ICONS.globe
               : item.kind === 'ask' ? ICONS.ai
               : ICONS.search;
    return html;
  }

  function kindLabelFor(item) {
    if (item.kind === 'tab') return 'Tab';
    if (item.kind === 'history') return 'History';
    if (item.kind === 'url') return 'URL';
    if (item.kind === 'ask') return 'AI';
    return 'Search';
  }

  function esc(s) {
    return String(s)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  function cleanHost(url) {
    return url.replace(/^https?:\/\//, '').replace(/\/$/, '') || url;
  }

  function fuzzyFilter(q, list) {
    // fuzzysort handles space-separated tokens natively (like fzf)
    const results = fuzzysort.go(q, list, {
      keys: ['title', 'url'],
      limit: 50,
    });

    // Re-sort with visit count and kind boosts
    const scored = results.map(r => {
      const item = r.obj;
      const fzScore = r.score;  // 0 (perfect) to -Infinity (worst)
      const visits = item.visit_count || 0;
      const visitBoost = visits > 0 ? Math.log2(visits + 1) * 0.05 : 0;
      const kindBoost = item.kind === 'tab' ? 0.3 : item.kind === 'history' ? 0.05 : 0;
      return { item, score: fzScore + visitBoost + kindBoost };
    });

    scored.sort((a, b) => b.score - a.score);
    return scored.map(s => s.item);
  }
})();
</script>
</body>
</html>"#
}
