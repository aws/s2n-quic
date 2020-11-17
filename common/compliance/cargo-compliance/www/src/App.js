import { useState, default as React } from "react";
import { makeStyles } from "@material-ui/core/styles";
import CssBaseline from "@material-ui/core/CssBaseline";
import Container from "@material-ui/core/Container";
import { Switch, Route, useParams } from "react-router-dom";
import { Nav } from "./nav";
import { Spec, Stats } from "./spec";
import { Section } from "./section";
import { Link } from "./link";
import specifications from "./result";
import clsx from "clsx";

const drawerWidth = 400;

const useStyles = makeStyles((theme) => ({
  root: {
    display: "flex",
  },
  appBar: {
    transition: theme.transitions.create(["margin", "width"], {
      easing: theme.transitions.easing.sharp,
      duration: theme.transitions.duration.leavingScreen,
    }),
  },
  appBarShift: {
    width: `calc(100% - ${drawerWidth}px)`,
    marginLeft: drawerWidth,
    transition: theme.transitions.create(["margin", "width"], {
      easing: theme.transitions.easing.easeOut,
      duration: theme.transitions.duration.enteringScreen,
    }),
  },
  menuButton: {
    marginRight: theme.spacing(2),
  },
  hide: {
    display: "none",
  },
  drawer: {
    width: drawerWidth,
    flexShrink: 0,
  },
  drawerPaper: {
    width: drawerWidth,
  },
  drawerHeader: {
    display: "flex",
    alignItems: "center",
    padding: theme.spacing(0, 1),
    // necessary for content to be below app bar
    ...theme.mixins.toolbar,
    justifyContent: "flex-end",
  },
  content: {
    flexGrow: 1,
    padding: theme.spacing(3),
    transition: theme.transitions.create("margin", {
      easing: theme.transitions.easing.sharp,
      duration: theme.transitions.duration.leavingScreen,
    }),
    marginLeft: -drawerWidth,
  },
  contentShift: {
    transition: theme.transitions.create("margin", {
      easing: theme.transitions.easing.easeOut,
      duration: theme.transitions.duration.enteringScreen,
    }),
    marginLeft: 0,
  },
  container: {
    marginBottom: theme.spacing(5),
  },
}));

function App() {
  const classes = useStyles();
  const [open, setOpen] = useState(false);

  return (
    <div className={classes.root}>
      <CssBaseline />
      <Nav open={open} setOpen={setOpen} />
      <main
        className={clsx(classes.content, {
          [classes.contentShift]: open,
        })}
      >
        <div className={classes.drawerHeader} />
        <Container maxWidth="lg" className={classes.container}>
          <Switch>
            <Route path="/spec/:specid/:sectionid">
              <SectionRoute />
            </Route>
            <Route path="/spec/:specid">
              <SpecRoute />
            </Route>
            <Route path="/">
              <Main />
            </Route>
          </Switch>
        </Container>
      </main>
    </div>
  );
}

function Main() {
  return specifications
    .filter((spec) => spec.stats.overall.total)
    .map((spec) => (
      <div key={spec.id}>
        <Link to={spec.url}>
          <h2>{spec.title}</h2>
        </Link>
        <Stats spec={spec} />
      </div>
    ));
}

function SpecRoute() {
  const { specid } = useParams();
  const spec = specifications[specid];
  if (!spec) return "spec not found";
  return <Spec spec={spec} />;
}

function SectionRoute() {
  const { specid, sectionid } = useParams();

  const spec = specifications[specid];
  if (!spec) return "spec not found";

  const section = spec.sections[sectionid];
  if (!section) return "section not found";

  return <Section spec={spec} section={section} />;
}

export default App;
