module.exports = {
  apps: [{
    name: "everylotbot-chicago",
    script: "/home/ubuntu/bots/everylotbot-chicago/venv/bin/python",
    args: ["-m", "everylot.bot"],
    cron_restart: "*/15 0,1,2,3,4,5,6,7,12,13,14,15,16,17,18,19,20,21,22,23 * * *",
    exec_mode: "fork",
    autorestart: false,
    watch: false,
    env: {
      TZ: "America/Chicago",
      PYTHONPATH: "/home/ubuntu/bots/everylotbot-chicago",
      PYTHONUNBUFFERED: "1",
      VIRTUAL_ENV: "/home/ubuntu/bots/everylotbot-chicago/venv"
    },
    cwd: "/home/ubuntu/bots/everylotbot-chicago",
    merge_logs: true
  }]
};
