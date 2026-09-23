// The real Send handler must consume slash commands, not send them to the LLM.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const app = fs.readFileSync(__dirname + '/../static/app.js', 'utf8');
const part = (start, end) => app.slice(app.indexOf(start), app.indexOf(end, app.indexOf(start)));
(async () => {
  for (const line of ['/stop', '/stop hier noch etwas', ' /STOP\nund mehr ', '/stop\t"hier"']) {
    const input = {value: line, style:{}};
    let stops = 0; const network = [];
    const sandbox = {console, chatUserId:'synthetic', chatAttachments:[], chatTtsOn:false,
      SLASH_COMMANDS:[{name:'/stop', action:async () => {stops++;}}],
      document:{getElementById: id => id === 'chat-input' ? input : {style:{}}},
      apiFetch:async url => {network.push(url); return {json:async()=>({})};},
      startChatStream(){}, renderAttachments(){}, startChatTimer(){}, addChatMessage(){}, autoScrollChat(){},
    };
    vm.createContext(sandbox);
    if (app.includes('async function dispatchChatCommand(')) vm.runInContext(part('async function dispatchChatCommand(', 'function runSlashCommand('), sandbox);
    vm.runInContext(part('async function chatSendMessage()', 'function autoScrollChat()'), sandbox);
    await sandbox.chatSendMessage();
    assert.equal(stops, 1, line); assert.deepEqual(network, [], 'command was sent as user prompt');
    assert.equal(input.value, '');
  }
  console.log('PASS: Send consumes /stop with trailing text, mixed case, tabs and newlines');
})().catch(e => {console.error(e); process.exitCode=1;});
