import { makeStyles } from "@material-ui/core/styles";
import Table from "@material-ui/core/Table";
import TableBody from "@material-ui/core/TableBody";
import TableCell from "@material-ui/core/TableCell";
import TableHead from "@material-ui/core/TableHead";
import TableRow from "@material-ui/core/TableRow";
import { Link } from "./link";

export function Spec({ spec }) {
  // todo sort and filter based on state
  const requirements = spec.requirements.map((requirement) => {
    return <Requirement requirement={requirement} key={requirement.id} />;
  });

  return (
    <>
      <h2>{spec.title}</h2>

      <h3>Stats</h3>
      <Stats spec={spec} />

      <h3>Requirements</h3>
      <Table size="small">
        <TableHead>
          <TableRow>
            <TableCell component="th">Section</TableCell>
            <TableCell>Requirement</TableCell>
            <TableCell>Text</TableCell>
            <TableCell>Status</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>{requirements}</TableBody>
      </Table>
    </>
  );
}

export function Stats({ spec: { stats } }) {
  return (
    <>
      <Table size="small">
        <TableHead>
          <TableRow>
            <TableCell component="th">Requirement</TableCell>
            <TableCell>Total</TableCell>
            <TableCell>Complete</TableCell>
            <TableCell>Complete %</TableCell>
            <TableCell>Citations</TableCell>
            <TableCell>Citations %</TableCell>
            <TableCell>Tests</TableCell>
            <TableCell>Exceptions</TableCell>
            <TableCell>TODOs</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          <StatsRow title="MUST" stats={stats.MUST} />
          <StatsRow title="SHALL" stats={stats.SHALL} />
          <StatsRow title="SHOULD" stats={stats.SHOULD} />
          <StatsRow title="MAY" stats={stats.MAY} />
          <StatsRow title="RECOMMENDED" stats={stats.RECOMMENDED} />
          <StatsRow title="OPTIONAL" stats={stats.OPTIONAL} />
          {/* TODO add a divider */}
          <StatsRow title="Totals" stats={stats.overall} />
        </TableBody>
      </Table>
    </>
  );
}

function StatsRow({ title, stats }) {
  return (
    <TableRow>
      <TableCell component="th">{title}</TableCell>
      <TableCell>{stats.total}</TableCell>
      <TableCell>{stats.complete}</TableCell>
      <TableCell>{stats.completePercent}</TableCell>
      <TableCell>{stats.citations}</TableCell>
      <TableCell>{stats.citationPercent}</TableCell>
      <TableCell>{stats.tests}</TableCell>
      <TableCell>{stats.exceptions}</TableCell>
      <TableCell>{stats.todos}</TableCell>
    </TableRow>
  );
}

const useStyles = makeStyles((theme) => ({
  error: {
    background: theme.palette.error.light,
    color: theme.palette.error.contrastText,
  },
  warning: {
    background: theme.palette.warning.light,
    color: theme.palette.warning.contrastText,
  },
  success: {
    background: theme.palette.success.light,
    color: theme.palette.success.contrastText,
  },
  info: {
    background: theme.palette.info.light,
    color: theme.palette.info.contrastText,
  },
}));

function Requirement({ requirement }) {
  const [status, cls] = requirementStatus(requirement);
  const classes = useStyles();
  return (
    <TableRow className={classes[cls]}>
      <TableCell align="left">
        <Link to={requirement.section.url}>{requirement.section.id}</Link>
      </TableCell>
      <TableCell>{requirement.level}</TableCell>
      <TableCell align="right">{requirement.comment}</TableCell>
      <TableCell>{status}</TableCell>
    </TableRow>
  );
}

function requirementStatus(requirement) {
  if (requirement.isComplete) return ["Complete", "success"];
  if (requirement.isOk) return ["Complete (with exceptions)", "info"];
  if (requirement.incomplete === requirement.spec)
    return ["Not started", "error"];

  if (requirement.spec === requirement.citation)
    return ["Missing test", "warning"];
  if (requirement.spec === requirement.test)
    return ["Missing citation", "warning"];

  return ["In progress", "warning"];
}
