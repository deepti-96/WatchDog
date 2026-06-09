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
    const service = req.body?.service || 'checkout';
    const scenario = service === 'payments' ? 'payments-latency' : 'checkout-timeout';
    const incident = autonomouslyTriageIncident(createScenarioIncident(scenario));
    await writeIncident(incident);
    sendJson(res, 200, {
      deployment_id: incident.verdict.deploy_id,
      incident_id: incident.id,
      autonomous_actions: incident.autonomous_run.actions,
      incident,
    });
  } catch (error) {
    sendError(res, error);
  }
};
