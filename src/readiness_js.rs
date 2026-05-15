//! JavaScript readiness probe used after navigation and by `browser_wait`.
//!
//! Goal: tell the AI when the page has actually rendered, while still resolving
//! on pages that *never* go fully quiet (live tickers, chat feeds, dashboards,
//! small CSS animations).
//!
//! # Signals
//!
//! All four must hold simultaneously:
//!
//! 1. `document.readyState === 'complete'` — base lifecycle gate.
//! 2. **LCP settled** — no new `largest-contentful-paint` candidate for
//!    ≥ 500 ms. LCP is purpose-built for "the main content is on screen".
//! 3. **DOM quiet OR steady-state** — ≤ 3 *structural* mutations (nodes
//!    added/removed) in the last 500 ms, OR the mutation rate has been
//!    roughly flat over the last 2 s. The steady-state fallback is what
//!    handles live dashboards / chat / tickers without hanging.
//! 4. **No long task** (> 50 ms main-thread block) in the last 300 ms.
//!
//! # Why MutationObserver is `childList + subtree` only
//!
//! Watching `attributes` or `characterData` makes every CSS class toggle,
//! hover state, animated counter, and live clock register as "busy". They
//! never mean "content is still arriving" — only added/removed nodes do.
//!
//! # Resolution values
//!
//! - `"ready"`   — fully settled.
//! - `"live"`    — steady-state mutation rate; treat as ready (this *is* the
//!   rendered shape, waiting longer won't help).
//! - `"partial"` — 8 s ceiling reached. Returns *something* on pathological
//!   sites instead of hanging the navigate.

pub const READINESS_JS: &str = r#"
new Promise(function(r){
  var s={lcp:0,lng:0,muts:[],t0:performance.now(),done:false};
  var now=function(){return performance.now();};
  var lcpO,lngO,mo;
  try{lcpO=new PerformanceObserver(function(l){var e=l.getEntries();for(var i=0;i<e.length;i++)s.lcp=now();});lcpO.observe({type:'largest-contentful-paint',buffered:true});}catch(e){}
  try{lngO=new PerformanceObserver(function(l){var e=l.getEntries();for(var i=0;i<e.length;i++)s.lng=now();});lngO.observe({type:'longtask',buffered:true});}catch(e){}
  mo=new MutationObserver(function(rs){
    var t=now();
    for(var i=0;i<rs.length;i++){var x=rs[i];if(x.addedNodes.length||x.removedNodes.length)s.muts.push(t);}
    while(s.muts.length&&t-s.muts[0]>2000)s.muts.shift();
  });
  mo.observe(document.body||document.documentElement,{childList:true,subtree:true});
  function finish(reason){
    if(s.done)return;s.done=true;
    try{if(lcpO)lcpO.disconnect();}catch(e){}
    try{if(lngO)lngO.disconnect();}catch(e){}
    try{mo.disconnect();}catch(e){}
    r(reason);
  }
  function tick(){
    if(s.done)return;
    var n=now();
    var elapsed=n-s.t0;
    if(elapsed>8000){finish('partial');return;}
    if(document.readyState==='complete'){
      var recent=0,older=0;
      for(var i=0;i<s.muts.length;i++){var dt=n-s.muts[i];if(dt<500)recent++;if(dt>1000)older++;}
      var newer=s.muts.length-older;
      var steady=s.muts.length>=4&&newer>=older*0.7;
      var domOk=recent<=3||steady;
      var lcpOk=s.lcp?(n-s.lcp)>=500:elapsed>=1000;
      var lngOk=!s.lng||(n-s.lng)>=300;
      if(domOk&&lcpOk&&lngOk){finish(steady&&recent>3?'live':'ready');return;}
    }
    setTimeout(tick,200);
  }
  setTimeout(tick,200);
});
"#;
