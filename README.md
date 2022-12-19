# KV Store API

This is a proof of concept quality project intended to experiment with in memory concurrency optimized data structures. This example makes use of [Axum] (https://docs.rs/axum/latest/axum/) 
and [evmap] (https://docs.rs/evmap/latest/evmap/index.html)
to build an API that allows CRUD operations on an in memory KV store. The major challenge of using an in memory data structure as a store is supporting concurrent reads/writes potentially across multiple threads with limited latency.
In an attempt to achieve this goal I am passing a write handle wrapped in a Mutex [parking_lot] (https://docs.rs/parking_lot/0.12.1/parking_lot/index.html)  and a read handle factory to all crud handlers. Both of these implementations will block while waiting to acquire the lock however, this introduces a minimal amount of latency when compared to operations on a standard HashMap when the number of keys exceed 10 million. 

To minimize potential performance bottlenecks as the backing store grows I have implemented evmap in an eventually consistent manner. A call such as:

```bash
curl -X POST localhost:3000/9996 -H "Content-Type: application/json" --data 1
```

will not attempt to directly write to the backing store. Instead the operation is placed into a queue of pending operations. Then at a standard interval a separate background task will reconcile all pending transactions and commit them to the 
evmap. This improves overall performance but could have two potential side effects. Since this background task must also acquire the writer lock should it take an excessive amount of time to execute it could potentially deadlock all other handlers
attempting to mutate the evmap. It is also possible (though unlikely) that users would be able to commit a value to the backing store then not immediately be able to fetch the value.

## TTL
In order to facilitate a rudimentary ttl for each key in the evmap a [priority_queue] (https://docs.rs/priority-queue/latest/priority_queue/) is used when in the same background task that reconciles the evmap. If a ttl is set via a call such as:

```bash
curl -X POST localhost:3000/9996/10 -H "Content-Type: application/json" --data 1
```

The operation will be added to the queue of pending operations. But in the reconciliation loop the pending transactions are evaluated. If a new value has a ttl its key will be added to the priority queue and its ttl value (in milliseconds) will
be added to the current DateTime then inserted as the value for the key. The priority queue is then checked to see if its smallest value is lower than the current time. If so the associated key is queued to be removed from the evmap.

## Benchmarks
 
To approximate the overall performance of the api [Artillery] (https://www.artillery.io/) is being used. It can be installed with

```bash
npm install -g artillery
```

Then to run the benchmark in one terminal start the server

```bash
cargo run --release
```

and in another terminal

```bash
artillery run bench.yaml
```

This will start a benchmark script set to call post on the server 10 million times over 10 minutes. It has been configured to fail if the 99th percentile exceeds 5 milliseconds or the 95th exceeds 1 millisecond 
(though I have noticed there are a few erroneous issues with artillery so these benchmarks are rather heuristic). Output for a single minute summery looks similar to this

```console
errors.an URL must be specified: ............................................... 27442
http.codes.200: ................................................................ 27441
http.request_rate: ............................................................. 2745/sec
http.requests: ................................................................. 27441
http.response_time:
  min: ......................................................................... 0
  max: ......................................................................... 152
  median: ...................................................................... 0
  p95: ......................................................................... 1
  p99: ......................................................................... 3
http.responses: ................................................................ 27442
vusers.created: ................................................................ 27441
vusers.created_by_name.0: ...................................................... 27441
vusers.failed: ................................................................. 27442
```

## Testing

Since the api is only a thin wrapper around the lib only the lib currently has tests the can be run with

```bash
cd kv-store-lib
cargo test
```
