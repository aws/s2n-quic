import { useState, default as React, useMemo, forwardRef } from "react";
import { makeStyles, useTheme } from "@material-ui/core/styles";
import clsx from "clsx";
import Drawer from "@material-ui/core/Drawer";
import AppBar from "@material-ui/core/AppBar";
import Toolbar from "@material-ui/core/Toolbar";
import List from "@material-ui/core/List";
import Typography from "@material-ui/core/Typography";
import Divider from "@material-ui/core/Divider";
import IconButton from "@material-ui/core/IconButton";
import MenuIcon from "@material-ui/icons/Menu";
import ChevronLeftIcon from "@material-ui/icons/ChevronLeft";
import ChevronRightIcon from "@material-ui/icons/ChevronRight";
import ListItem from "@material-ui/core/ListItem";
import ListItemText from "@material-ui/core/ListItemText";
import Collapse from "@material-ui/core/Collapse";
import ExpandLess from "@material-ui/icons/ExpandLess";
import ExpandMore from "@material-ui/icons/ExpandMore";
import { useRouteMatch } from "react-router-dom";
import specifications from "./result";
import { Link } from "./link";

const drawerWidth = 400;

const useStyles = makeStyles((theme) => ({
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
    position: "fixed",
    top: 0,
    left: 0,
    alignItems: "center",
    width: drawerWidth,
    padding: theme.spacing(0, 1),
    // necessary for content to be below app bar
    ...theme.mixins.toolbar,
    display: "flex",
    justifyContent: "flex-end",
    backgroundColor: theme.palette.background.paper,
    borderBottom: `1px solid ${theme.palette.divider}`,
    zIndex: 1,
  },
  drawerContent: {
    marginTop: theme.spacing(7),
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
  nested: {
    paddingLeft: theme.spacing(4),
  },
  sectionTitle: {
    paddingLeft: theme.spacing(4),
    textAlign: "right",
  },
}));

export function Nav({ open, setOpen }) {
  const classes = useStyles();
  const theme = useTheme();

  const handleDrawerOpen = () => {
    setOpen(true);
  };

  const handleDrawerClose = () => {
    setOpen(false);
  };

  return (
    <>
      <AppBar
        position="fixed"
        className={clsx(classes.appBar, {
          [classes.appBarShift]: open,
        })}
      >
        <Toolbar>
          <IconButton
            color="inherit"
            aria-label="open drawer"
            onClick={handleDrawerOpen}
            edge="start"
            className={clsx(classes.menuButton, open && classes.hide)}
          >
            <MenuIcon />
          </IconButton>
          <Link to="/" color="inherit">
            <Typography variant="h6" noWrap>
              Compliance Coverage Report
            </Typography>
          </Link>
        </Toolbar>
      </AppBar>
      <Drawer
        className={classes.drawer}
        variant="persistent"
        anchor="left"
        open={open}
        classes={{
          paper: classes.drawerPaper,
        }}
      >
        <div className={classes.drawerHeader}>
          <IconButton onClick={handleDrawerClose}>
            {theme.direction === "ltr" ? (
              <ChevronLeftIcon />
            ) : (
              <ChevronRightIcon />
            )}
          </IconButton>
        </div>
        <List className={classes.drawerContent}>
          {specifications.map((spec, index) => (
            <SpecItem spec={spec} key={index} />
          ))}
        </List>
      </Drawer>
    </>
  );
}

// create all of the section elements up front for better performance
const specSections = specifications.reduce((acc, spec) => {
  acc[spec.id] = spec.sections.map((section, index) => (
    <SectionItem section={section} key={index} />
  ));
  return acc;
}, {});

function SpecItem({ spec }) {
  const selected = !!useRouteMatch(spec.url);
  const [open, setOpen] = useState(selected);
  const handleClick = (evt) => {
    if (selected) {
      setOpen(!open);
      evt.preventDefault();
    } else {
      setOpen(true);
    }
  };
  const handleMore = (evt) => {
    setOpen(true);
    evt.stopPropagation();
    evt.preventDefault();
  };
  const handleLess = (evt) => {
    setOpen(false);
    evt.stopPropagation();
    evt.preventDefault();
  };
  const sections = specSections[spec.id];

  return (
    <>
      <Divider light />
      <ListItemLink
        button
        selected={selected}
        to={spec.url}
        onClick={handleClick}
      >
        <ListItemText primary={spec.title} />
        {open ? (
          <ExpandLess onClick={handleLess} />
        ) : (
          <ExpandMore onClick={handleMore} />
        )}
      </ListItemLink>
      <Collapse in={open} timeout={100} unmountOnExit>
        <List component="div" disablePadding>
          {sections}
        </List>
      </Collapse>
    </>
  );
}

function SectionItem({ spec, section }) {
  const classes = useStyles();
  const selected = !!useRouteMatch(section.url);
  return (
    <ListItemLink
      button
      className={classes.nested}
      selected={selected}
      to={section.url}
      key={section.id}
    >
      <ListItemText secondary={`${section.id}`} />
      <ListItemText
        className={classes.sectionTitle}
        secondary={`${section.title}`}
      />
    </ListItemLink>
  );
}

function ListItemLink(props) {
  const { to, ...rest } = props;

  const CustomLink = useMemo(
    () =>
      forwardRef((linkProps, ref) => <Link ref={ref} to={to} {...linkProps} />),
    [to]
  );

  return (
    <li>
      <ListItem component={CustomLink} {...rest} />
    </li>
  );
}
