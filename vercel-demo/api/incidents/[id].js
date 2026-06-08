const { readIncident, sendError, sendJson } = require('../_lib/watchdog');

module.exports = async function handler(req, res) {
  if (req.method !== 'GET') {
    sendJson(res, 405, { error: 'method not allowed' });
    return;
  }
  try {
    const incident = await readIncident(req.query.id);
    if (!incident) {
      sendJson(res, 404, { error: 'incident not found' });
      return;
    }
    sendJson(res, 200, incident);
  } catch (error) {
    sendError(res, error);
  }
};
