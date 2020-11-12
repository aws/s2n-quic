import { makeStyles } from "@material-ui/core/styles";
import { DataGrid } from "@material-ui/data-grid";
import Table from "@material-ui/core/Table";
import TableBody from "@material-ui/core/TableBody";
import TableCell from "@material-ui/core/TableCell";
import TableHead from "@material-ui/core/TableHead";
import TableRow from "@material-ui/core/TableRow";
import Tooltip from "@material-ui/core/Tooltip";
import { Link } from "./link";

export function Spec({ spec }) {
  return (
    <>
      <h2>{spec.title}</h2>

      <h3>Stats</h3>
      <Stats spec={spec} />

      <h3>Requirements</h3>
      <Requirements requirements={spec.requirements} showSection />
    </>
  );
}

const useStyles = makeStyles((theme) => ({
  root: {
    "& > div": {
      // fix the weird inline style height
      height: "auto !important",
    },
  },
  text: {
    lineHeight: "initial !important",
    padding: theme.spacing(2, 1),
    whiteSpace: "normal !important",
    overflow: "auto !important",
  },
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

const LEVELS = ["MUST", "SHALL", "SHOULD", "MAY", "RECOMMENDED", "OPTIONAL"];

const LEVEL_IDS = LEVELS.reduce((acc, level, idx) => {
  acc[level] = idx;
  return acc;
}, {});

export function Requirements({ requirements, showSection }) {
  const classes = useStyles();

  const columns = [];

  if (showSection) {
    columns.push({
      field: "section",
      headerName: "Section",
      valueGetter(params) {
        return params.data.section.id;
      },
      sortComparator(v1, v2, row1, row2) {
        return row1.data.cmp(row2.data);
      },
      renderCell(params) {
        const requirement = params.data;
        return (
          <Link
            to={{
              pathname: requirement.section.url,
              hash: `#A${requirement.id}`,
            }}
          >
            {requirement.section.id}
          </Link>
        );
      },
    });
  }

  columns.push(
    ...[
      {
        field: "level",
        headerName: "Requirement",
        width: 120,
        sortComparator(v1, v2) {
          return LEVEL_IDS[v2] - LEVEL_IDS[v1];
        },
      },
      {
        field: "status",
        headerName: "Status",
        width: 150,
        sortComparator(v1, v2, row1, row2) {
          const a = requirementStatus(row1.data)[0];
          const b = requirementStatus(row2.data)[0];
          return b - a;
        },
        valueGetter(params) {
          return requirementStatus(params.data) || [];
        },
        valueFormatter(params) {
          return params.value[1];
        },
        cellClassName(params) {
          return classes[params.value[2]];
        },
      },
      {
        field: "comment",
        headerName: "Text",
        sortable: false,
        width: 850,
        cellClassName: classes.text,
      },
    ]
  );

  return (
    <div className={classes.root}>
      <DataGrid
        pageSize={25}
        disableSelectionOnClick
        autoHeight={true}
        rows={requirements}
        columns={columns}
      />
    </div>
  );
}

export function Stats({ spec: { stats } }) {
  return (
    <>
      <Table size="small">
        <TableHead>
          <TableRow>
            <TableCell component="th">Requirement</TableCell>
            <TableCell align="right">Total</TableCell>
            <TableCell align="right">Complete</TableCell>
            <TableCell align="right">Citations</TableCell>
            <TableCell align="right">Tests</TableCell>
            <TableCell align="right">Exceptions</TableCell>
            <TableCell align="right">TODOs</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {LEVELS.filter((level) => stats[level].total).map((level) => (
            <StatsRow key={level} title={level} stats={stats[level]} />
          ))}
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
      <TableCell align="right">{stats.total}</TableCell>
      <TableCell align="right">
        <Tooltip title={stats.completePercent}>
          <span>{stats.complete}</span>
        </Tooltip>
      </TableCell>
      <TableCell align="right">
        <Tooltip title={stats.citationPercent}>
          <span>{stats.citations}</span>
        </Tooltip>
      </TableCell>
      <TableCell align="right">{stats.tests}</TableCell>
      <TableCell align="right">{stats.exceptions}</TableCell>
      <TableCell align="right">{stats.todos}</TableCell>
    </TableRow>
  );
}

function requirementStatus(requirement) {
  if (requirement.isComplete) return [1, "Complete", "success"];
  if (requirement.isOk) return [2, "Complete (with exceptions)", "info"];
  if (requirement.incomplete === requirement.spec)
    return [7, "Not started", "error"];

  if (requirement.spec === requirement.citation)
    return [6, "Missing test", "warning"];
  if (requirement.spec === requirement.test)
    return [5, "Missing citation", "warning"];

  return [4, "In progress", "warning"];
}
