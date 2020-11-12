const input = JSON.parse(document.getElementById("result").innerHTML);
//import input from "./result.test.json";

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

const linker = createLinker(input.blob_link);
input.annotations.forEach((anno, id) => {
  const status = input.statuses[id];
  if (status) {
    status.related = (status.related || []).map((id) => input.annotations[id]);
    Object.assign(anno, status);
    anno.isComplete = anno.spec === anno.citation && anno.spec === anno.test;
    anno.isOk = anno.isComplete || anno.exception === anno.spec;
  }

  anno.id = id;
  anno.source = linker(anno);
  anno.specification = specifications[anno.target_path];
  anno.section = anno.specification.sections[`section-${anno.target_section}`];
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
    if (requirement.todo) this.exceptions += 1;
  }

  get completePercent() {
    return this.percent(this.complete);
  }

  get citationPercent() {
    return this.percent(this.citations);
  }

  percent(value) {
    const percent = this.total ? value / this.total : 0;
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
  });

  const stats = {
    overall: new Stats(),
    MUST: new Stats(),
    SHALL: new Stats(),
    SHOULD: new Stats(),
    MAY: new Stats(),
    RECOMMENDED: new Stats(),
    OPTIONAL: new Stats(),
  };

  spec.requirements.forEach((requirement) => {
    stats.overall.onRequirement(requirement);
    let s = stats[requirement.level] || new Stats();
    stats[requirement.level] = s;
    s.onRequirement(requirement);
  });

  spec.stats = stats;
});

function sortRequirements(a, b) {
  return a.cmp(b);
}

function createLinker(blob_link) {
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
    };
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
