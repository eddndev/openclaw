import subprocess
import time
import requests
import os
import signal
import sys
import json

COMMANDER_PORT = 24999
BASE_URL = f"http://localhost:{COMMANDER_PORT}"

def log(msg):
    print(f"[TEST] {msg}", flush=True)

def wait_for_api(retries=30):
    for i in range(retries):
        try:
            requests.get(f"{BASE_URL}/status")
            return True
        except requests.exceptions.ConnectionError:
            time.sleep(1)
    return False

def get_status():
    res = requests.get(f"{BASE_URL}/status")
    return res.json()

def test_fleet():
    log("Building Commander...")
    build = subprocess.run(
        ["cargo", "build", "--manifest-path", "commander/Cargo.toml"], 
        capture_output=True
    )
    if build.returncode != 0:
        log("Build failed!")
        print(build.stderr.decode())
        sys.exit(1)

    log("Starting Fleet (1 agent)...")
    # Set env vars for the test
    env = os.environ.copy()
    env["COMMANDER_FLEET_ID"] = "test-fleet"
    env["COMMANDER_BASE_PORT"] = "25000" # Use different ports to avoid conflicts
    env["RUST_LOG"] = "info"

    # Start commander in background
    commander_proc = subprocess.Popen(
        ["cargo", "run", "--manifest-path", "commander/Cargo.toml", "--", "start-fleet", "--count", "1"],
        env=env,
        stdout=sys.stdout, # Inherit to see logs directly in this terminal
        stderr=sys.stderr,
        preexec_fn=os.setsid # Create new session group
    )

    try:
        if not wait_for_api():
            log("Commander API did not start in time.")
            sys.exit(1)
        
        log("API is UP. Checking Agent Status...")
        status = get_status()
        assert len(status) == 1
        agent_0 = status[0]
        assert agent_0["id"] == "test-fleet-0"
        assert agent_0["status"] == "Running"
        initial_pid = agent_0["pid"]
        log(f"Agent 0 is RUNNING (PID: {initial_pid})")

        # --- Test 1: Lifecycle Stop ---
        log("--- Test 1: Lifecycle Stop ---")
        res = requests.post(f"{BASE_URL}/agents/test-fleet-0/stop")
        assert res.status_code == 200
        time.sleep(2) # Wait for stop to propagate
        
        status = get_status()
        assert status[0]["status"] == "Stopped"
        log("Agent stopped successfully via API.")

        # --- Test 2: Lifecycle Start ---
        log("--- Test 2: Lifecycle Start ---")
        requests.post(f"{BASE_URL}/agents/test-fleet-0/start")
        time.sleep(2)
        
        status = get_status()
        assert status[0]["status"] == "Running"
        new_pid = status[0]["pid"]
        assert new_pid != initial_pid
        log(f"Agent started successfully (New PID: {new_pid}).")

        # --- Test 3: Lifecycle Restart ---
        log("--- Test 3: Lifecycle Restart ---")
        requests.post(f"{BASE_URL}/agents/test-fleet-0/restart")
        time.sleep(2)
        
        status = get_status()
        assert status[0]["status"] == "Running"
        restarted_pid = status[0]["pid"]
        assert restarted_pid != new_pid
        log(f"Agent restarted successfully (PID: {restarted_pid}).")

        # --- Test 4: Watchdog (Crash Simulation) ---
        log("--- Test 4: Watchdog (Crash Simulation) ---")
        current_pid = restarted_pid
        log(f"Killing Agent Process {current_pid}...")
        os.kill(current_pid, signal.SIGKILL)
        
        # Immediate check (might be Failed or Restarting)
        time.sleep(0.5)
        # Wait for watchdog to kick in (backoff logic might delay it slightly, but initial restart should be fast)
        log("Waiting for watchdog...")
        time.sleep(3) 

        status = get_status()
        final_pid = status[0]["pid"]
        final_status = status[0]["status"]
        
        log(f"Current Status: {final_status}, PID: {final_pid}")
        assert final_status == "Running"
        assert final_pid != current_pid
        log("Watchdog successfully revived the agent!")

        log("ALL TESTS PASSED ✅")

    except Exception as e:
        log(f"TEST FAILED: {e}")
    finally:
        log("Cleaning up...")
        os.killpg(os.getpgid(commander_proc.pid), signal.SIGTERM)
        commander_proc.wait()

if __name__ == "__main__":
    test_fleet()
