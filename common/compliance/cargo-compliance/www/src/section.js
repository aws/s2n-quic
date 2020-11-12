import { useState, useMemo, default as React } from "react";
import { useLocation } from "react-router-dom";
import { makeStyles, withStyles } from "@material-ui/core/styles";
import Box from "@material-ui/core/Box";
import List from "@material-ui/core/List";
import Link from "@material-ui/core/Link";
import ListItem from "@material-ui/core/ListItem";
import ListItemText from "@material-ui/core/ListItemText";
import Paper from "@material-ui/core/Paper";
import Dialog from "@material-ui/core/Dialog";
import Tooltip from "@material-ui/core/Tooltip";
import clsx from "clsx";
import { Requirements } from "./spec";

export function Section({ spec, section }) {
  const requirements = section.requirements || [];
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
          <Requirements requirements={section.requirements} showSection />
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
    margin: theme.spacing(1),
    paddingTop: theme.spacing(1),
    paddingLeft: theme.spacing(2),
    paddingRight: theme.spacing(2),
    paddingBottom: theme.spacing(2),
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
          className={clsx(classes[statusClass], {
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
  };

  annotations.forEach((anno) => {
    if (anno.source) {
      (refs[anno.type || "CITATION"] || []).push(anno);
    }
  });

  const requirement = status.level ? <h3>Level: {status.level}</h3> : null;

  const comments = expanded
    ? annotations.filter((ref) => ref.type === "SPEC" && ref.comment)
    : [];

  return (
    <>
      {requirement}
      {comments.map((anno, i) => (
        <p key={anno.id}>{anno.comment}</p>
      ))}
      <AnnotationRef title="Defined in" refs={refs.SPEC} expanded={expanded} />
      <AnnotationRef
        title="Implemented in"
        alt={requirement && "Missing implementation!"}
        refs={refs.CITATION}
        expanded={expanded}
      />
      <AnnotationRef
        title="Tested in"
        alt={requirement && "Missing test!"}
        refs={refs.TEST}
        expanded={expanded}
      />
      <AnnotationRef
        title="Exempted in"
        refs={refs.EXCEPTION}
        expanded={expanded}
      />
      <AnnotationRef
        title="To be implemented in"
        refs={refs.TODO}
        expanded={expanded}
      />
    </>
  );
}

function AnnotationRef({ title, alt, refs, expanded }) {
  if (!refs.length)
    return alt ? (
      <Box color="error.main">
        <h4>{alt}</h4>
      </Box>
    ) : null;

  return (
    <>
      <h4>{title}</h4>
      <List>
        {refs.map((anno, id) => {
          const text = <ListItemText secondary={anno.source.title} />;
          const content = anno.source.href ? (
            <Link href={anno.source.href}>{text}</Link>
          ) : (
            text
          );
          return <ListItem key={id}>{content}</ListItem>;
        })}
      </List>
    </>
  );
}
