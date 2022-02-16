const fs = require("fs/promises");
const Zip = require("adm-zip");

const ci = {
  compliance: async () => {
    await fs.rename(
      "artifacts/compliance/report.html",
      "reports/compliance.html"
    );

    await sanitize("reports/compliance.html");

    return {
      target_url: `${cdn}/compliance.html`,
    };
  },

  coverage: async () => {
    await fs.rename("artifacts/coverage", "reports/coverage");

    await sanitize("reports/coverage");

    return {
      target_url: `${cdn}/coverage/index.html`,
    };
  },

  doc: async () => {
    await fs.rename("artifacts/doc", "reports/doc");
    await sanitize("reports/doc");

    return {
      target_url: `${cdn}/doc/s2n_quic/index.html`,
    };
  },

  "recovery-sim": async () => {
    // TODO
  },

  timing: async () => {
    await fs.rename("artifact/timing/cargo-timing.html", "reports/timing.html");
    await sanitize("reports/timing.html");

    return {
      target_url: `${cdn}/timing.html`,
    };
  },
};

const qns = {
  bench: async ({ cdn }) => {
    await fs.rename("artifacts/bench", "reports/bench");
    await sanitize("reports/bench", { extensions: ["html", "svg"] });

    return {
      target_url: `${cdn}/bench/index.html`,
    };
  },

  interop: async () => {
    // TODO
  },

  perf: async () => {
    // TODO
  },
};

async function sanitize(
  path,
  {
    extensions = ["html", "js", "css", "svg"],
    max_bytes = 1000000,
    max_files = 100,
  }
) {
  extensions = new Set(extensions);

  // TODO walk dir and remove files that don't fit the criteria
}

const workflows = { ci, qns };

const requested = async (args) => {
  const { context, github, workflow } = args;

  const reports = Object.keys(workflow);

  const submitStatus = async (report) => {
    await github.repos.createCommitStatus({
      context: `report / ${report}`,
      owner: context.repo.owner,
      repo: context.repo.repo,
      sha: context.payload.workflow_run.head_sha,
      state: "pending",
    });
  };

  await Promise.all(reports.map(submitStatus));
};

const completed = async (args) => {
  const { context, github, workflow } = args;

  const reports = Object.keys(workflow);

  const {
    data: { artifacts },
  } = await github.rest.actions.listWorkflowRunArtifacts({
    owner: context.repo.owner,
    repo: context.repo.repo,
    run_id: context.payload.workflow_run.id,
    per_page: 100,
  });

  await fs.mkdir("artifacts");

  const downloadArtifact = async ({ id, name }) => {
    const download = await github.rest.actions.downloadArtifact({
      owner: context.repo.owner,
      repo: context.repo.repo,
      artifact_id: id,
      archive_format: "zip",
    });

    const path = `artifacts/${name}`;

    const zip = new Zip(Buffer.from(download.data));
    zip.extractAllTo(path);

    return path;
  };

  const downloads = artifacts
    .filter((artifact) =>
      reports.some((prefix) => artifact.name.startsWith(prefix))
    )
    .map((artifact) => downloadArtifact(artifact));

  const paths = await Promise.all(downloads);

  args.artifacts = paths;

  const submitStatus = async (report) => {
    let res = {};

    try {
      res = await workflow[report](args);
    } catch (error) {
      res = { state: "failure" };
      console.error(report, "report failed with error: ", error);
    }

    const sha = context.payload.workflow_run.head_sha;

    if (!sha) {
      console.log("done", report, res);
      return;
    }

    await github.repos.createCommitStatus({
      context: report,
      owner: context.repo.owner,
      repo: context.repo.repo,
      sha,
      state: "success",
      ...(res || {}),
    });
  };

  await Promise.all(reports.map(submitStatus));
};

const actions = { requested, completed };

module.exports = async (args) => {
  const { context, core, github } = args;

  // mock out the payload for tests
  if (context.payload.pull_request) {
    // pull the most recent workflow from main
    const { data: results } = await github.rest.actions.listWorkflowRunsForRepo(
      {
        owner: context.repo.owner,
        repo: context.repo.repo,
        event: "push",
        status: "success",
      }
    );

    const workflow = results.workflow_runs.find((res) => res.name == "qns");

    args.context.payload = {
      action: "completed",
      workflow: {
        name: workflow.name,
      },
      workflow_run: {
        id: workflow.id,
        // TODO
        // head_sha: workflow.head_sha,
      },
    };
  }

  const { payload } = context;

  const workflow = workflows[payload.workflow.name];
  if (!workflow)
    return core.warning(`unhandled workflow: ${payload.workflow.name}`);

  const action = actions[payload.action];
  if (!action) return core.warning(`unhandled action: ${payload.action}`);

  const cdn = `${process.env.CDN}/${payload.workflow_run.head_sha}`;

  await action({ workflow, cdn, ...args });
};
