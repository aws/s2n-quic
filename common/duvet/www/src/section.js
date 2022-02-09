import { useState, useMemo, default as React } from "react";
import { useLocation } from "react-router-dom";
import { makeStyles, withStyles } from "@material-ui/core/styles";
import Box from "@material-ui/core/Box";
import List from "@material-ui/core/List";
import ListItem from "@material-ui/core/ListItem";
import ListItemText from "@material-ui/core/ListItemText";
import Paper from "@material-ui/core/Paper";
import Dialog from "@material-ui/core/Dialog";
import Tooltip from "@material-ui/core/Tooltip";
import Button from "@material-ui/core/Button";
import ButtonGroup from "@material-ui/core/ButtonGroup";
import Select from "@material-ui/core/Select";
import MenuItem from "@material-ui/core/MenuItem";
import Typography from "@material-ui/core/Typography";
import clsx from "clsx";
import copyToClipboard from "copy-to-clipboard";
import { Requirements } from "./spec";
import { Link } from "./link";

export function Section({ spec, section }) {
  const requirements = section.requirements || [];
  // the datagrid is crashing on link transition and needs to be rebuilt
  const key = `${spec.id}--${section.id}`;
  return (
    <>
      <h2>
        {section.id} {section.title}
      </h2>
      <pre>
        {section.lines.map((line, i) => (
          <Line content={line} key={i} />
        ))}
      </pre>
      {requirements.length ? (
        <>
          <h3>Requirements</h3>
          <Requirements
            key={key}
            requirements={section.requirements}
            showSection
          />
        </>
      ) : null}
    </>
  );
}

function Line({ content }) {
  return (
    <>
      {content.map((reference, i) => {
        // don't highlight text without references
        if (!reference.annotations.length) return reference.text;

        return <Quote reference={reference} key={i} />;
      })}
      <br />
    </>
  );
}

const useStyles = makeStyles((theme) => ({
  paper: {
    margin: theme.spacing(2),
    padding: theme.spacing(2),
  },
  reference: {
    cursor: "pointer",
  },
  error: {
    borderBottom: `2px solid ${theme.palette.error.main}`,
  },
  warning: {
    borderBottom: `2px solid ${theme.palette.warning.main}`,
  },
  ok: {
    borderBottom: `2px solid ${theme.palette.success.main}`,
  },
  neutral: {
    borderBottom: `2px solid ${theme.palette.info.main}`,
  },
  exception: {
    borderBottom: `2px solid ${theme.palette.text.disabled}`,
  },
  selected: {
    backgroundColor: theme.palette.action.focus,
  },
}));

const QuoteTooltip = withStyles((theme) => ({
  tooltip: {
    backgroundColor: theme.palette.background.paper,
    color: theme.palette.text.primary,
    maxWidth: 512,
    fontSize: theme.typography.pxToRem(12),
    border: "1px solid #dadde9",
  },
}))(Tooltip);

function useAnnotationSelection() {
  const { hash } = useLocation();

  return useMemo(() => {
    return new Set(
      (hash || "")
        .replace(/^#/, "")
        .split(",")
        .filter((id) => /^A[\d]+/.test(id))
        .map((id) => parseInt(id.slice(1)))
    );
  }, [hash]);
}

function Quote({ reference }) {
  const { status, text } = reference;
  const classes = useStyles();
  const [open, setOpen] = useState(false);
  const selectedAnnotations = useAnnotationSelection();

  const handleOpen = () => {
    setOpen(true);
  };
  const handleClose = () => {
    setOpen(false);
  };

  let statusClass = "neutral";

  if (status.spec) {
    if (status.citation && status.test) {
      statusClass = "ok";
    } else if (status.citation || status.test) {
      statusClass = "warning";
    } else {
      statusClass = "error";
    }
  }

  if (status.exception) {
    statusClass = "exception";
  }

  let selected =
    selectedAnnotations.size &&
    reference.annotations.find((anno) => selectedAnnotations.has(anno.id));

  return (
    <>
      <QuoteTooltip title={<Annotations reference={reference} />}>
        <span
          className={clsx(classes.reference, classes[statusClass], {
            [classes.selected]: selected,
          })}
          onClick={handleOpen}
        >
          {text}
        </span>
      </QuoteTooltip>
      <Dialog open={open} onClose={handleClose} maxWidth={false}>
        <Paper className={classes.paper}>
          <Annotations reference={reference} expanded />
        </Paper>
      </Dialog>
    </>
  );
}

function Annotations({ reference: { annotations, status }, expanded }) {
  const refs = {
    CITATION: [],
    SPEC: [],
    TEST: [],
    EXCEPTION: [],
    TODO: [],
    features: new Set(),
    tracking_issues: new Set(),
    tags: new Set(),
  };

  annotations.forEach((anno) => {
    if (anno.source) {
      (refs[anno.type || "CITATION"] || []).push(anno);
    }
    anno.features.forEach(refs.features.add, refs.features);
    anno.tracking_issues.forEach(
      refs.tracking_issues.add,
      refs.tracking_issues
    );
    anno.tags.forEach(refs.tags.add, refs.tags);
  });

  const requirement = status.level ? <h3>Level: {status.level}</h3> : null;
  const isOk = !!refs.SPEC.find((ref) => ref.isOk);
  const showMissing = requirement && !isOk;

  const comments = expanded ? refs.SPEC.filter((ref) => ref.comment) : [];

  return (
    <>
      {requirement}
      {comments.map((anno, i) => (
        <Comment annotation={anno} key={anno.id} />
      ))}
      {expanded ? (
        <AnnotationList title="Features" items={refs.features} />
      ) : null}
      {expanded ? (
        <AnnotationList title="Tracking issues" items={refs.tracking_issues} />
      ) : null}
      {expanded ? <AnnotationList title="Tags" items={refs.tags} /> : null}
      <AnnotationRef
        title="Specifications"
        refs={refs.SPEC.length > 1 ? refs.SPEC : []}
      />
      <AnnotationRef
        title="Citations"
        alt={showMissing && "Missing!"}
        refs={refs.CITATION}
      />
      <AnnotationRef
        title="Tests"
        alt={showMissing && "Missing!"}
        refs={refs.TEST}
      />
      <AnnotationRef
        title="Exceptions"
        refs={refs.EXCEPTION}
        expanded={expanded}
      />
      <AnnotationRef title="TODOs" refs={refs.TODO} />
    </>
  );
}

const listItemStyle = { padding: "0 8px", display: "block" };

function AnnotationList({ title, items }) {
  if (!items.size) return null;
  items = Array.from(items);

  if (items.length === 1) {
    // remove `s` if there's only 1
    title = title.slice(0, title.length - 1);
  } else {
    // sort the items if there's more than 1
    items.sort();
  }

  const content = items.map((item, idx) => {
    const text = item.toString();
    const content = item.href ? <Link href={item.href}>{text}</Link> : text;
    return (
      <ListItem style={{ ...listItemStyle, display: "inline" }} key={idx}>
        {idx ? ", " : ""}
        {content}
      </ListItem>
    );
  });

  return (
    <div>
      <h3 style={{ lineHeight: 1, display: "inline" }}>{title}</h3>
      <List style={{ padding: 0, display: "inline" }}>{content}</List>
    </div>
  );
}

function AnnotationRef({ title, alt, refs, expanded }) {
  if (!refs.length && !alt) {
    return null;
  }

  const content = refs.length ? (
    refs.map((anno, id) => {
      const text = <ListItemText secondary={anno.source.title} />;
      const content = anno.source.href ? (
        <Link href={anno.source.href}>{text}</Link>
      ) : (
        text
      );
      return (
        <ListItem style={listItemStyle} key={id}>
          {content}
          {expanded && anno.comment ? (
            <Typography
              variant="body2"
              style={{ paddingLeft: 16, maxWidth: 500 }}
            >
              {anno.comment}
            </Typography>
          ) : null}
        </ListItem>
      );
    })
  ) : (
    <ListItem style={listItemStyle}>
      <Box color="error.main">
        <h4>{alt}</h4>
      </Box>
    </ListItem>
  );

  return (
    <>
      <h4 style={{ lineHeight: 1 }}>{title}</h4>
      <List style={{ padding: 0 }}>{content}</List>
    </>
  );
}

const useCommentStyles = makeStyles((theme) => ({
  cite: {
    display: "flex",
    flexWrap: "wrap",
    justifyContent: "right",
    marginBottom: "2em",
  },
}));

function Comment({ annotation }) {
  const classes = useCommentStyles();
  const [format, setFormat] = useState("comment");

  const formatComment = {
    toml: formatTomlComment,
    comment: formatReferenceComment,
  }[format];

  const newIssueLink = annotation.newIssueLink();

  return (
    <>
      <p>
        <pre>{annotation.comment}</pre>
      </p>
      <div className={classes.cite}>
        <Select
          value={format}
          onChange={(event) => setFormat(event.target.value)}
          autoWidth
        >
          <MenuItem value={"comment"}>Comment</MenuItem>
          <MenuItem value={"toml"}>Toml</MenuItem>
        </Select>
        <ButtonGroup size="small" color="primary" variant="contained">
          {[
            { label: "Citation", type: "citation" },
            { label: "Test", type: "test" },
            { label: "Exception", type: "exception" },
            { label: "TODO", type: "TODO" },
          ].map(({ label, type }) => (
            <Cite
              key={label}
              getData={() => formatComment({ annotation, type })}
              label={label}
            />
          ))}
          {newIssueLink && (
            <Button href={newIssueLink} target="_blank">
              Issue
            </Button>
          )}
        </ButtonGroup>
      </div>
    </>
  );
}

function Cite({ getData, label, ...props }) {
  const [copied, setCopied] = useState(false);

  const onClick = () => {
    copyToClipboard(getData());
    setCopied(true);
    setTimeout(() => setCopied(false), 1000);
  };

  return (
    <Button onClick={onClick} {...props}>
      {copied ? `${label} - Copied!` : label}
    </Button>
  );
}

function formatReferenceComment({ annotation, type }) {
  let comment = [];
  comment.push(`//= ${annotation.target}`);
  if (type !== "citation") comment.push(`//= type=${type}`);
  annotation.comment
    .trim()
    .split("\n")
    .forEach((line) => {
      comment.push(`//# ${line}`);
    });
  return comment.join("\n") + "\n";
}

function formatTomlComment({ annotation, type }) {
  let comment = [`[[${type}]]`];
  comment.push(`target = ${JSON.stringify(annotation.target)}`);
  comment.push("quote = '''");
  comment.push(...annotation.comment.trim().split("\n"));
  comment.push("'''");
  if (type === "exception") {
    comment.push("reason = '''");
    comment.push("TODO: Add reason for exception here");
    comment.push("'''");
  }

  return comment.join("\n") + "\n";
}
