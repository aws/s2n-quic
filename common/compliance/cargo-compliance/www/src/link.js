import { Link as A } from "react-router-dom";
import B from "@material-ui/core/Link";

export function Link(props) {
  return <B component={A} {...props} />;
}
