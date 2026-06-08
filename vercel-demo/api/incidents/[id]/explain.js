const { explainIncident, readIncident, sendError, sendJson, writeIncident } = require('../../_lib/watchdog');

module.exports = async function handler(req, res) {
  if (req.method !== 'POST') {
    sendJson(res, 405, { error: 'method not allowed' });
    return;
  }
  try {
    const incident = await readIncident(req.query.id);
    if (!incident) {
      sendJson(res, 404, { error: 'incident not found' });
      return;
    }
    const explanation = explainIncident(incident);
    incident.cached_explanation = explanation;
    incident.cached_explanation_updated_at = new Date().toISOString();
    await writeIncident(incident);
    sendJson(res, 200, { explanation });
  } catch (error) {
    sendError(res, error);
  }
};
