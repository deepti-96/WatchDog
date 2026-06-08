const { listIncidents, listItem, sendError, sendJson } = require('./_lib/watchdog');

module.exports = async function handler(req, res) {
  if (req.method !== 'GET') {
    sendJson(res, 405, { error: 'method not allowed' });
    return;
  }
  try {
    const incidents = await listIncidents();
    sendJson(res, 200, incidents.map(listItem));
  } catch (error) {
    sendError(res, error);
  }
};
