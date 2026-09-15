// Official Webots R2025a RobotWindow API, matching the installed SDK examples.
import RobotWindow from 'https://cyberbotics.com/wwi/R2025a/RobotWindow.js';
const robotWindow = new RobotWindow();
robotWindow.setTitle('Talk to NAO');
robotWindow.receive = message => {
  if (message.startsWith('nao_chat_session ')) {
    const url = new URL(message.slice(17));
    if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || url.username || url.password) return;
    document.getElementById('chat').src = url.href;
    document.getElementById('chat').hidden = false;
    document.getElementById('state').hidden = true;
  } else if (message === 'nao_chat_unavailable') document.getElementById('state').textContent = 'Start scripts/run_nao_social.py --sim webots to enable this conversation window.';
};
robotWindow.send('nao_chat_ready');
