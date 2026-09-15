(function () {
  'use strict';
  const el = id => document.getElementById(id);
  const Recognition = window.SpeechRecognition || window.webkitSpeechRecognition;
  let token = '', sequence = 0, generation = 0, pending = '', busy = false;
  let controller = null, recognition = null, timer = null, modality = 'typed', capture = 0, confidence = 1;
  let lastPresentation = '';
  const state = text => { el('state').textContent = text; };
  function controls() {
    el('message').disabled = !token || busy;
    el('send').disabled = !token || busy;
    el('mic').disabled = !token || busy || !Recognition || !window.isSecureContext || !el('voice-consent').checked;
  }
  function line(who, text) {
    const li = document.createElement('li');
    li.className = who === 'NAO' ? 'nao' : 'player';
    li.textContent = who + ': ' + text;
    el('history').appendChild(li);
    while (el('history').children.length > 40) el('history').firstChild.remove();
    li.scrollIntoView({ block: 'nearest' });
  }
  async function request(path, data, credential = token, signal) {
    const abort = new AbortController(), cancel = () => abort.abort();
    if (signal) { if (signal.aborted) cancel(); signal.addEventListener('abort', cancel, {once:true}); }
    const deadline = setTimeout(cancel, 5000);
    try {
      const response = await fetch(path, {method: 'POST', headers: {'Content-Type':'application/json', 'Authorization':'Bearer '+credential}, body:JSON.stringify(data), signal:abort.signal});
      const raw = await response.text();
      if (raw.length > 16384) throw Error('Conversation response exceeds bounds');
      const result = JSON.parse(raw);
      if (!response.ok) throw Error(result.error || 'Conversation request failed');
      return result;
    } finally { clearTimeout(deadline); if (signal) signal.removeEventListener('abort', cancel); }
  }
  function stopCapture() {
    if (recognition) { recognition.onresult = null; recognition.onerror = null; recognition.onend = null; recognition.abort(); recognition = null; }
    el('mic').textContent = 'Dictate';
    if (window.speechSynthesis) window.speechSynthesis.cancel();
  }
  function stop() {
    const oldToken = token;
    generation++; token = ''; busy = false; pending = ''; lastPresentation = '';
    clearTimeout(timer); if (controller) controller.abort(); controller = null;
    stopCapture(); el('voice-consent').checked = false; el('speak').checked = false;
    controls(); state('Conversation stopped. Join again to continue.');
    if (oldToken) request('/api/leave', {}, oldToken).catch(() => {});
  }
  async function poll(epoch) {
    if (epoch !== generation || !pending) return;
    try {
      const result = await request('/api/poll', {id:pending}, token, controller.signal);
      if (epoch !== generation) return;
      if (result.state === 'queued' || result.state === 'active') {
        state(result.state === 'queued' ? 'Waiting for NAO’s attention…' : 'NAO is receiving your message…');
        timer = setTimeout(() => poll(epoch), 200); return;
      }
      if (result.state === 'replied' && result.reply && result.reply.presentation_id !== lastPresentation) {
        lastPresentation = result.reply.presentation_id;
        line('NAO', result.reply.text);
        // A same-origin simulator may show the same neural reply above its body.
        if (window.parent !== window) window.parent.postMessage({type:'nao-reply', reply:result.reply}, window.location.origin);
        if (el('speak').checked && window.speechSynthesis) {
          const voice = new SpeechSynthesisUtterance(result.reply.text); voice.lang = 'en-GB'; window.speechSynthesis.speak(voice);
        }
        state('Your turn.');
      } else state(result.state + '. No NAO speech was generated.');
      pending = ''; busy = false; controls(); el('message').focus();
    } catch (error) { if (epoch === generation) { stop(); state(error.message + ' Rejoin to continue.'); } }
  }
  el('connect').onclick = async () => {
    stop(); const epoch = generation;
    el('connect').disabled = true;
    try {
      const result = await request('/api/join', {}, el('invite').value.trim());
      if (epoch !== generation) return;
      token = result.player_token; sequence = 0; el('invite').value = '';
      state('Connected to NAO. Type a message.'); controls(); el('message').focus();
      controller = new AbortController(); busy = true; controls();
      const encounter = await request('/api/encounter', {kind:'player',sequence:sequence++,capture_ns:Math.round(performance.now()*1e6)}, token, controller.signal);
      if (epoch !== generation) return;
      pending = encounter.id;
      if (pending) poll(epoch); else { busy = false; controls(); }
    } catch (error) { if (epoch === generation) state(error.message); }
    finally { el('connect').disabled = false; }
  };
  el('form').onsubmit = async event => {
    event.preventDefault(); if (!token || busy) return;
    const text = el('message').value;
    if (!text.trim() || new TextEncoder().encode(text).length > 256) { state('Use 1–256 UTF-8 bytes.'); return; }
    stopCapture(); const epoch = generation; busy = true; controls();
    controller = new AbortController();
    try {
      const result = await request('/api/turn', {text, sequence:sequence++, capture_ns:capture || Math.round(performance.now()*1e6), modality, confidence}, token, controller.signal);
      if (epoch !== generation) return;
      line('You', text); el('message').value = ''; modality = 'typed'; capture = 0; confidence = 1;
      pending = result.id; state('Message queued.'); poll(epoch);
    } catch (error) { if (epoch === generation) { busy = false; controls(); state(error.message); } }
  };
  el('message').oninput = () => { modality = 'typed'; capture = 0; confidence = 1; };
  el('voice-capability').textContent = Recognition && window.isSecureContext
    ? 'Dictation creates an editable transcript. Audio is not captured until you press Dictate.'
    : 'Dictation is unavailable in this browser. You can type or use your operating system’s dictation.';
  el('voice-consent').disabled = !Recognition || !window.isSecureContext;
  el('speak').disabled = !window.speechSynthesis;
  el('voice-consent').onchange = () => { if (!el('voice-consent').checked) stopCapture(); controls(); };
  el('speak').onchange = () => { if (!el('speak').checked && window.speechSynthesis) window.speechSynthesis.cancel(); };
  el('mic').onclick = () => {
    if (recognition) { stopCapture(); return; }
    if (el('mic').disabled) return;
    const epoch = generation; capture = Math.round(performance.now()*1e6);
    recognition = new Recognition(); recognition.lang = 'en-GB'; recognition.continuous = false; recognition.interimResults = false;
    recognition.onresult = event => {
      if (epoch !== generation) return;
      el('message').value = event.results[0][0].transcript; confidence = event.results[0][0].confidence;
      modality = 'speech_transcript'; state('Review the transcript, then press Send.');
    };
    recognition.onerror = () => { if (epoch === generation) state('Dictation failed or permission was denied. You can still type.'); };
    recognition.onend = () => { if (epoch === generation) { recognition = null; el('mic').textContent = 'Dictate'; } };
    try { recognition.start(); el('mic').textContent = 'Stop dictation'; state('Microphone active for dictation…'); }
    catch (_) { stopCapture(); state('Dictation unavailable. You can still type.'); }
  };
  el('stop').onclick = stop;
  window.addEventListener('pagehide', stop);
  document.addEventListener('visibilitychange', () => { if (document.hidden) stopCapture(); });
  const fragment = new URLSearchParams(location.hash.slice(1));
  if (fragment.has('invite')) { el('invite').value = fragment.get('invite'); history.replaceState(null, '', location.pathname); }
  controls();
}());
