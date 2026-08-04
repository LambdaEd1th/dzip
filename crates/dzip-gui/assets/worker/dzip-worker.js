const hardwareConcurrency = Math.max(1, self.navigator?.hardwareConcurrency ?? 1);
const workerCount = Math.min(4, Math.max(1, hardwareConcurrency - 1));
const backend = "worker-pool";

let nextWorkerCursor = 0;

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
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

class RuntimeWorker {
  constructor(index) {
    this.index = index;
    this.nextRequestId = 1;
    this.pending = new Map();
    this.failed = false;
    this.worker = new Worker(
      new URL("./dzip-worker-runtime.js", import.meta.url),
      { type: "module" },
    );
    this.worker.onmessage = (event) => this.handleMessage(event.data);
    this.worker.onerror = (event) => {
      this.fail(
        new Error(event.message || `Dzip runtime worker ${index + 1} failed`),
      );
    };
  }

  get pendingCount() {
    return this.pending.size;
  }

  handleMessage(message) {
    const pending = this.pending.get(message.id);
    if (!pending) return;
    this.pending.delete(message.id);
    if (message.ok) {
      pending.resolve(message.response);
    } else {
      pending.reject(new Error(message.error || "Dzip runtime worker failed"));
    }
  }

  fail(error) {
    this.failed = true;
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    this.pending.clear();
  }

  request(request) {
    if (this.failed) {
      return Promise.reject(
        new Error(`Dzip runtime worker ${this.index + 1} is unavailable`),
      );
    }
    const id = this.nextRequestId;
    this.nextRequestId = (this.nextRequestId + 1) || 1;
    const payload = { id, request };
    const transfer = collectTransferables(payload);
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      try {
        this.worker.postMessage(payload, transfer);
      } catch (error) {
        this.pending.delete(id);
        reject(error);
      }
    });
  }
}

const runtimeWorkers = Array.from(
  { length: workerCount },
  (_, index) => new RuntimeWorker(index),
);

function runtimeWorker(index) {
  if (runtimeWorkers[index].failed) {
    runtimeWorkers[index].worker.terminate();
    runtimeWorkers[index] = new RuntimeWorker(index);
  }
  return runtimeWorkers[index];
}

function chooseWorker() {
  let selectedIndex = nextWorkerCursor;
  let selectedPending = Number.POSITIVE_INFINITY;
  for (let offset = 0; offset < workerCount; offset += 1) {
    const index = (nextWorkerCursor + offset) % workerCount;
    const pending = runtimeWorker(index).pendingCount;
    if (pending < selectedPending) {
      selectedIndex = index;
      selectedPending = pending;
    }
  }
  nextWorkerCursor = (selectedIndex + 1) % workerCount;
  return runtimeWorker(selectedIndex);
}

self.onmessage = async (event) => {
  const { id, request } = event.data;
  try {
    const response = await chooseWorker().request(request);
    self.postMessage(
      {
        id,
        ok: true,
        response,
        backend,
        threadCount: workerCount,
      },
      collectTransferables(response),
    );
  } catch (error) {
    self.postMessage({
      id,
      ok: false,
      error: errorMessage(error),
      backend,
      threadCount: workerCount,
    });
  }
};
