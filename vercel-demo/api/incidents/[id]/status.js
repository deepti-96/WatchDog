const { readIncident, sendError, sendJson, writeIncident } = require('../../_lib/watchdog');

module.exports = async function handler(req, res) {
  if (req.method !== 'POST') {
    sendJson(res, 405, { error: 'method not allowed' });
    return;
  }
  try {
    const status = String(req.body?.status || '').toLowerCase();
    if (!['open', 'resolved'].includes(status)) {
      sendJson(res, 400, { error: 'invalid status' });
      return;
    }
    const incident = await readIncident(req.query.id);
    if (!incident) {
      sendJson(res, 404, { error: 'incident not found' });
      return;
    }
    incident.status = status;
    await writeIncident(incident);
    sendJson(res, 200, incident);
  } catch (error) {
    sendError(res, error);
  }
};
