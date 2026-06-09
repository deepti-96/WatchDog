const {
  autonomouslyTriageIncident,
  createScenarioIncident,
  sendError,
  sendJson,
  writeIncident,
} = require('../_lib/watchdog');

module.exports = async function handler(req, res) {
  if (req.method !== 'POST') {
    sendJson(res, 405, { error: 'method not allowed' });
    return;
  }
  try {
    const scenario = req.body?.scenario || 'checkout-timeout';
    const incident = autonomouslyTriageIncident(createScenarioIncident(scenario));
    await writeIncident(incident);
    sendJson(res, 200, { scenario, incident_id: incident.id, autonomous_actions: incident.autonomous_run.actions, incident });
  } catch (error) {
    sendError(res, error);
  }
};
