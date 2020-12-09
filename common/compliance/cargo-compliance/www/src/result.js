const input =
  process.env.NODE_ENV === "production"
    ? JSON.parse(document.getElementById("result").innerHTML)
    : require("./result.test.json");

const specifications = [];

Object.keys(input.specifications).forEach((id) => {
  const spec = input.specifications[id];

  const parts = id.split("/");
  const title = spec.title || parts[parts.length - 1].replace(".txt", "");
  const url = `/spec/${encodeURIComponent(id)}`;

  const sections = [];
  spec.sections.forEach((section, idx) => {
    section.url = `${url}/section-${encodeURIComponent(section.id)}`;
    section.lines = section.lines.map(mapLine);
    section.spec = spec;
    section.idx = idx;
    section.requirements = (section.requirements || []).map(
      (id) => input.annotations[id]
    );

    sections.push(section);
    sections[`section-${section.id}`] = section;
    sections[`section-${encodeURIComponent(section.id)}`] = section;
  });

  const s = {
    id,
    title,
    url,
    sections,
    requirements: spec.requirements.map((id) => input.annotations[id]),
  };

  specifications.push(s);
  specifications[id] = s;
  specifications[encodeURIComponent(id)] = s;
});

const blobLinker = createBlobLinker(input.blob_link);
const issueLinker = createIssueLinker(input.issue_link);
const newIssueLinker = createNewIssueLinker(input.issue_link);
input.annotations.forEach((anno, id) => {
  const status = input.statuses[id];
  if (status) {
    status.related = (status.related || []).map((id) => input.annotations[id]);
    Object.assign(anno, status);
    anno.isComplete = anno.spec === anno.citation && anno.spec === anno.test;
    anno.isOk = anno.isComplete || anno.exception === anno.spec;
  }

  anno.id = id;
  anno.source = blobLinker(anno);
  anno.specification = specifications[anno.target_path];
  anno.section = anno.specification.sections[`section-${anno.target_section}`];
  anno.target = `${anno.specification.id}#${anno.section.id}`;
  anno.features = [];
  anno.tracking_issues = [];
  anno.tags = anno.tags || [];
  anno.newIssueLink = newIssueLinker;
  anno.cmp = function (b) {
    const a = this;
    if (a.specification === b.specification && a.section.idx !== b.section.idx)
      return a.section.idx - b.section.idx;
    return a.id - b.id;
  };
});

class Stats {
  constructor() {
    this.total = 0;
    this.complete = 0;
    this.incomplete = 0;
    this.citations = 0;
    this.tests = 0;
    this.exceptions = 0;
    this.todos = 0;
  }

  onRequirement(requirement) {
    this.total += 1;

    if (requirement.incomplete) this.incomplete += 1;
    else if (requirement.isComplete) this.complete += 1;

    if (requirement.citation) this.citations += 1;
    if (requirement.test) this.tests += 1;
    if (requirement.exception) this.exceptions += 1;
    if (requirement.todo) this.todos += 1;
  }

  percent(field) {
    const percent = this.total ? this[field] / this.total : 0;
    return Number(percent).toLocaleString(undefined, {
      style: "percent",
      minimumFractionDigits: 0,
      maximumFractionDigits: 2,
    });
  }
}

// create stats now that we've linked everything
specifications.forEach((spec) => {
  spec.requirements.sort(sortRequirements);
  spec.sections.forEach((section) => {
    section.requirements.sort(sortRequirements);
    section.stats = getRequirementsStats(section.requirements);
  });

  spec.stats = getRequirementsStats(spec.requirements);
});

function getRequirementsStats(reqs) {
  const stats = {
    overall: new Stats(),
    MUST: new Stats(),
    SHALL: new Stats(),
    SHOULD: new Stats(),
    MAY: new Stats(),
    RECOMMENDED: new Stats(),
    OPTIONAL: new Stats(),
  };

  reqs.maxFeatures = 0;
  reqs.maxTrackingIssues = 0;
  reqs.maxTags = 0;

  reqs.forEach((requirement) => {
    stats.overall.onRequirement(requirement);
    let s = stats[requirement.level] || new Stats();
    stats[requirement.level] = s;
    s.onRequirement(requirement);
    const features = new Set();
    const tracking_issues = new Set();
    const tags = new Set();

    function onRelated(related) {
      if (related.feature) features.add(related.feature);
      if (related.tracking_issue) tracking_issues.add(related.tracking_issue);
      (related.tags || []).forEach(tags.add, tags);
    }

    onRelated(requirement);
    (requirement.related || []).forEach(onRelated);

    requirement.features = Array.from(features);
    requirement.features.sort();
    reqs.maxFeatures = Math.max(reqs.maxFeatures, features.size);

    requirement.tracking_issues = Array.from(tracking_issues);
    requirement.tracking_issues.sort();
    requirement.tracking_issues = requirement.tracking_issues.map(issueLinker);
    reqs.maxTrackingIssues = Math.max(
      reqs.maxTrackingIssues,
      tracking_issues.size
    );

    requirement.tags = Array.from(tags);
    requirement.tags.sort();
    reqs.maxTags = Math.max(reqs.maxTags, tags.size);
  });

  return stats;
}

function sortRequirements(a, b) {
  return a.cmp(b);
}

function createBlobLinker(blob_link) {
  blob_link = (blob_link || "").replace(/\/+$/, "");

  return (anno) => {
    if (!anno.source) return null;

    let link = anno.source;

    if (anno.line > 0) {
      link += `#L${anno.line}`;
    }

    if (anno.line > 0 && anno.line_impl > 0) {
      link += `-L${anno.line_impl}`;
    }

    return {
      title: link,
      href: blob_link.length ? `${blob_link}/${link}` : null,
      toString() {
        return link;
      },
    };
  };
}

function createIssueLinker(base) {
  base = (base || "").replace(/\/+$/, "");

  return (issue) => {
    if (!issue) return null;
    if (/^http(s)?:/.test(issue)) return { title: issue, href: issue };

    return {
      title: issue,
      href: base.length ? `${base}/${issue}` : null,
      toString() {
        return issue;
      },
    };
  };
}

function createNewIssueLinker(base) {
  base = (base || "").replace(/\/+$/, "");

  return function () {
    if (!base) return false;
    if (!this.comment) return false;

    const url = new URL(`${base}/new`);

    const quote = this.comment
      .trim()
      .split("\n")
      .map((line) => `> ${line}`)
      .join("\n");

    const body = `
From [${this.section.title}](${this.target}) in [${this.specification.title}](${this.target_path}):

${quote}`;

    url.searchParams.set("body", body);
    const labels = [
      "compliance",
      this.level && `compliance:${this.level}`,
      this.specification.title && `spec:${this.specification.title}`,
    ]
      .concat(this.features)
      .filter((l) => !!l)
      .join(",");
    url.searchParams.set("labels", labels);

    return url.toString();
  };
}

function mapLine(line) {
  if (typeof line === "string")
    return [{ annotations: [], status: input.refs[0], text: line }];

  return line.map((ref) => {
    if (typeof ref === "string")
      return { annotations: [], status: input.refs[0], text: ref };

    const [ids, status, text] = ref;
    const annotations = ids.map((id) => input.annotations[id]);
    return {
      annotations,
      status: input.refs[status] || input.refs[0],
      text,
    };
  });
}

export default specifications;
