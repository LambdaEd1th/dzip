const ready = (async () => {
  const runtime = await import("./pkg/dzip_gui_worker.js");
  await runtime.default();
  return runtime;
})();

function workflowError(error) {
  if (
    error !== null &&
    typeof error === "object" &&
    typeof error.code === "string" &&
    typeof error.message === "string"
  ) {
    return error;
  }
  return {
    code: "io",
    message: error instanceof Error ? error.message : String(error),
  };
}

function collectTransferables(
  value,
  transfer = [],
  seenValues = new Set(),
  seenBuffers = new Set(),
) {
  if (value === null || typeof value !== "object" || seenValues.has(value)) {
    return transfer;
  }
  seenValues.add(value);
  if (ArrayBuffer.isView(value)) {
    const buffer = value.buffer;
    if (buffer instanceof ArrayBuffer && !seenBuffers.has(buffer)) {
      seenBuffers.add(buffer);
      transfer.push(buffer);
    }
    return transfer;
  }
  if (value instanceof ArrayBuffer) {
    if (!seenBuffers.has(value)) {
      seenBuffers.add(value);
      transfer.push(value);
    }
    return transfer;
  }
  for (const child of Array.isArray(value) ? value : Object.values(value)) {
    collectTransferables(child, transfer, seenValues, seenBuffers);
  }
  return transfer;
}

self.onmessage = async (event) => {
  const { id, request } = event.data;
  try {
    const runtime = await ready;
    const response = runtime.dzip_worker_run(request);
    self.postMessage(
      { id, ok: true, response, backend: "stateful-worker", threadCount: 1 },
      collectTransferables(response),
    );
  } catch (error) {
    self.postMessage({
      id,
      ok: false,
      error: workflowError(error),
      backend: "stateful-worker",
      threadCount: 1,
    });
  }
};
