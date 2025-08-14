# job-watcher

`job-watcher` is a Rust library for running and monitoring periodic
background tasks. It provides a simple framework for defining,
configuring, and observing jobs, and can expose a web-based status
page to visualize their health.

## Features

*   **Periodic Task Scheduling**: Easily define tasks that run at regular intervals.
*   **Centralized Configuration**: Configure task-specific properties
    like execution delay, out-of-date thresholds, and retry logic.
*   **Health Monitoring**: Automatically tracks successes, failures,
    and whether a task has become "out of date" (i.e., not
    successfully completed within a configured timeframe).
*   **Web Status Page**: Serves an HTML status page showing the state
    of all watched tasks, including recent output, execution counts,
    and environment details.
*   **Custom Alerting**: Implement your own logic to determine when a
    task failure should trigger an alert.
*   **Stateful Tasks**: Tasks can access their own execution history
    (e.g., success/failure counts) via a `Heartbeat` mechanism to make
    stateful decisions.

## Quick Start

Here's how to get started with `job-watcher`

### 1. Define your Application Context

First, create a struct for your application and implement the
`WatcherAppContext` trait. This trait provides configuration and
top-level information to the watcher.

### 2. Define your Watched Tasks

Next, create structs for your background jobs and implement the
`WatchedTask` trait. The `run_single` method contains the logic for
your job.

Here we define a simple, stateless task, and a second, stateful task
that uses the `Heartbeat` to fail after a certain number

### 3. Build and Run the Watcher

Finally, in your `main` function, use the `AppBuilder` to register
your tasks and start the watcher. See the code under [examples](./examples/)
directory for more details.
