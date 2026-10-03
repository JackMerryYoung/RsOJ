import { useEffect, useState, useRef, useCallback } from "react";
import {
    makeStyles,
    Button,
    Label,
    Field,
    Input,
    Table,
    TableHeader,
    TableRow,
    TableHeaderCell,
    TableCell,
    TableBody,
    Spinner,
} from "@fluentui/react-components";
import { useLocation, useNavigate, useOutletContext } from "react-router-dom";
import { useTranslation } from "react-i18next";

import { useSelector } from "react-redux";

import * as globals from "../Globals.ts";
import { RootState } from "../store.ts";
import { DocumentCheckmarkRegular, ErrorCircle20Color } from "@fluentui/react-icons";

function getColorByResult(result: string) {
    if (result === "AC") return "#3AAF00";
    else if (result === "CE") return "#FDDB10";
    else if (result === "WA") return "#DA3737";
    else if (result === "UKE") return "#1F103F";
    else return "#4183C4";
}

function getColorByScore(score: number) {
    if (score >= 80) return "#3AAF00";
    else if (score >= 60) return "#FDDB10";
    else return "#DA3737";
}

const useStyles = makeStyles({
    root: {
        display: "flex",
        flexDirection: "column",
        rowGap: "0.5em",
        columnGap: "0.5em",
        width: "100%",
        margin: "0.5em 0 0.5em 0",
    },
});
interface SubmissionResult {
    submission_id: number;
    username: string;
    result: string;
    general_score: number;
    statuses: string[];
    scores: number[];
    problem_number: number;
}

interface SubmissionResultOthers {
    submission_id: number;
    username: string;
    result: string;
    problem_number: number;
}

type SubmissionListItem = SubmissionResult | SubmissionResultOthers;

function isSubmissionResultOwn(submission: SubmissionListItem): submission is SubmissionResult {
    return "general_score" in submission;
}

function useSubmissionsList(
    request: globals.WebSocketRequest,
    setElapsedTime: React.Dispatch<React.SetStateAction<number>>,
    username: string,
) {
    const [submissionsList, setSubmissionsList] = useState<SubmissionListItem[] | undefined>(undefined);

    const elapsedTimerSinceLoaded = useRef(0);

    useEffect(() => {
        elapsedTimerSinceLoaded.current = setInterval(() => {
            setElapsedTime(x => x + 1);
        }, 1000);

        return () => {
            if (elapsedTimerSinceLoaded.current) {
                clearInterval(elapsedTimerSinceLoaded.current);
            }
        };
    }, []);

    const loadSubmissionsList = useCallback(async (_submissionsListIndex: number, signal?: AbortSignal) => {
        try {
            const response = await request<{ type: string; content: { request_key: string; submissions_list?: SubmissionListItem[] } }>(
                "submissions_list",
                { index: _submissionsListIndex, username },
                { signal },
            );
            if (response.content.submissions_list) {
                setSubmissionsList(response.content.submissions_list);
            }
        } catch (error) {
            if (error instanceof Error && error.name === "AbortError") return;
        }
    }, [request, username]);

    return { submissionsList, loadSubmissionsList };
}

function useTotalSubmissionsListIndex(
    request: globals.WebSocketRequest,
    username: string,
) {
    const [totalSubmissionsListIndex, setTotalSubmissionsListIndex] = useState(1);

    const loadTotalSubmissionsListIndex = useCallback(async (signal?: AbortSignal) => {
        try {
            const response = await request<{ type: string; content: { request_key: string; total_submissions_list_index?: number } }>(
                "total_submissions_list_index",
                { username },
                { signal },
            );
            if (response.content.total_submissions_list_index !== undefined) {
                setTotalSubmissionsListIndex(response.content.total_submissions_list_index);
            }
        } catch (error) {
            if (error instanceof Error && error.name === "AbortError") return;
        }
    }, [request, username]);

    return { totalSubmissionsListIndex, loadTotalSubmissionsListIndex };
}

export default function SubmissionsList() {
    const [elapsedTime, setElapsedTime] = useState(0);
    const { request } = useOutletContext<globals.WebSocketHook>();
    const [submissionId, setSubmissionId] = useState("-1");
    const [username, setUsername] = useState("");
    const [submissionsListIndex, setSubmissionsListIndex] = useState(1);
    const loginStatus = useSelector((state: RootState) => state.loginStatus);
    const normalizedUsername = username.trim();
    const { totalSubmissionsListIndex, loadTotalSubmissionsListIndex } = useTotalSubmissionsListIndex(request, normalizedUsername)
    const { submissionsList, loadSubmissionsList } = useSubmissionsList(request, setElapsedTime, normalizedUsername)
    const navigate = useNavigate();
    const location = useLocation();
    const rootStyle = useStyles().root;
    const { t } = useTranslation("submissionsList");

    useEffect(() => {
        if (loginStatus.value !== true) return;
        const controller = new AbortController();
        void loadTotalSubmissionsListIndex(controller.signal);
        return () => controller.abort();
    }, [loadTotalSubmissionsListIndex, loginStatus.value]);
    useEffect(() => {
        if (loginStatus.value !== true) return;
        const controller = new AbortController();
        void loadSubmissionsList(submissionsListIndex, controller.signal);
        return () => controller.abort();
    }, [loadSubmissionsList, loginStatus.value, submissionsListIndex]);

    return <>
        {
            loginStatus.value &&
            (
                submissionsList === undefined ?
                    <div style={{ padding: "0.5em 0.5em 0 0.5em" }}>
                        {
                            elapsedTime >= 10
                                ?
                                <div style={{ display: "flex", blockSize: "100%" }}>
                                    <ErrorCircle20Color style={{ margin: "auto 0 0 auto" }} />
                                    <Label style={{ margin: "auto auto 0 0", padding: "0 0 0 8px", fontSize: "14px" }}>
                                        {t("failedToFetch")}
                                    </Label></div>
                                :
                                <Spinner size="large" label={t("waiting")} delay={500} />
                        }
                    </div>
                    :
                    <>
                        <div className={rootStyle}>
                            <form style={{ padding: "0 0.9em" }}>
                                <Field label={t("field.username")}>
                                    <Input
                                        value={username}
                                        onChange={(_event, data) => {
                                            setUsername(data.value);
                                            setSubmissionsListIndex(1);
                                        }}
                                        placeholder={t("field.usernamePlaceholder")}
                                        style={{ width: "100%" }} />
                                </Field>
                                <Field label={t("field.submissionId")} style={{ marginTop: "0.5em" }}>
                                    <div style={{ display: "flex", columnGap: "0.25em" }}>
                                        <Input onChange={(props) => setSubmissionId(props.target.value)} style={{ flex: "75%" }} />
                                        <Button size="medium" appearance="primary"
                                            style={{ flex: "0 0 30%", minWidth: 0, minHeight: "28px", height: "28px", padding: "0 0.35em", fontSize: "0.95em" }}
                                            onClick={() => { navigate("/submission/" + String(submissionId)); }}>{t("action.jumpTo")}</Button>
                                    </div>
                                </Field>
                                <div style={{ marginTop: "1em" }}>
                                    <div style={{ margin: "0 0 0.25em 0" }}>
                                        <Label>{t("indexPage", { current: submissionsListIndex, total: Math.max(totalSubmissionsListIndex, 1) })}</Label>
                                    </div>
                                    <div style={{ display: "flex", columnGap: "0.25em" }}>
                                        <Button size="medium" appearance="primary"
                                            style={{ flex: "1 1 25%", minWidth: 0, minHeight: "28px", height: "28px", padding: "0 0.25em", fontSize: "0.95em" }}
                                            onClick={() => { setSubmissionsListIndex((c) => (Math.max(1, c - 1))); }}>{t("action.previous")}</Button>
                                        <Button size="medium" appearance="primary"
                                            style={{ flex: "1 1 25%", minWidth: 0, minHeight: "28px", height: "28px", padding: "0 0.25em", fontSize: "0.95em" }}
                                            onClick={() => { setSubmissionsListIndex((c) => (Math.max(1, Math.min(totalSubmissionsListIndex, c + 1)))); }}>{t("action.next")}</Button>
                                        <Button size="medium" appearance="secondary"
                                            style={{ flex: "1 1 25%", minWidth: 0, minHeight: "28px", height: "28px", padding: "0 0.25em", fontSize: "0.95em" }}
                                            onClick={() => { setSubmissionsListIndex((c) => (Math.max(1, c - 10))); }}>{t("action.backward10")}</Button>
                                        <Button size="medium" appearance="secondary"
                                            style={{ flex: "1 1 25%", minWidth: 0, minHeight: "28px", height: "28px", padding: "0 0.25em", fontSize: "0.95em" }}
                                            onClick={() => { setSubmissionsListIndex((c) => (Math.max(1, Math.min(1, totalSubmissionsListIndex, c + 10)))); }}>{t("action.forward10")}</Button>
                                    </div>
                                </div>
                            </form>
                        </div>
                        <div style={{ margin: "0.6em 0.25em 0 0.75em", marginTop: "1em" }} >
                            <Table size="medium" className="scroll-bar-wrap" style={{ width: "100%" }}>
                                <TableHeader className="my-table-sticky">
                                    <TableRow className="my-table-row-header">
                                        <TableHeaderCell style={{ width: "20%" }} className="my-table-cell">{t("column.submissionId")}</TableHeaderCell>
                                        <TableHeaderCell style={{ width: "20%" }}>{t("column.username")}</TableHeaderCell>
                                        <TableHeaderCell style={{ width: "20%" }}>{t("column.problemNumber")}</TableHeaderCell>
                                        <TableHeaderCell style={{ width: "20%" }}>{t("column.status")}</TableHeaderCell>
                                        <TableHeaderCell style={{ width: "20%" }}>{t("column.score")}</TableHeaderCell>
                                    </TableRow>
                                </TableHeader>
                                <TableBody className="my-table-scrollbar">
                                    {
                                        submissionsList.map((submissionResult: SubmissionResult | SubmissionResultOthers) => (
                                            <TableRow key={submissionResult.submission_id} className="my-table-row-body" style={{ backgroundColor: location.pathname === "/submission/" + submissionResult.submission_id ? "#f0f6ff" : undefined }}>
                                                <TableCell style={{ color: "#4183C4", width: "20%" }} className="my-table-cell"
                                                    onMouseEnter={(e) => { (e.target as HTMLTableCellElement).style.color = "#0056B3"; (e.target as HTMLTableCellElement).style.cursor = "pointer"; }}
                                                    onMouseLeave={(e) => { (e.target as HTMLTableCellElement).style.color = "#4183C4"; (e.target as HTMLTableCellElement).style.cursor = "default"; }}
                                                    onClick={() => { navigate("/submission/" + String(submissionResult.submission_id)); }}>
                                                    <span style={{ display: "inline-flex", alignItems: "center", columnGap: "0.25em" }}>
                                                        {isSubmissionResultOwn(submissionResult) && (
                                                            <DocumentCheckmarkRegular
                                                                title={t("ownSubmission")}
                                                                aria-label={t("ownSubmission")}
                                                                fontSize="1em" />
                                                        )}
                                                        {submissionResult.submission_id}
                                                    </span>
                                                </TableCell>
                                                <TableCell style={{ width: "20%" }} className="my-table-cell">
                                                    {submissionResult.username}
                                                </TableCell>
                                                <TableCell style={{ color: "#4183C4", width: "20%" }} className="my-table-cell"
                                                    onMouseEnter={(e) => { (e.target as HTMLTableCellElement).style.color = "#0056B3"; (e.target as HTMLTableCellElement).style.cursor = "pointer"; }}
                                                    onMouseLeave={(e) => { (e.target as HTMLTableCellElement).style.color = "#4183C4"; (e.target as HTMLTableCellElement).style.cursor = "default"; }}
                                                    onClick={() => { navigate("/problem/" + String(submissionResult.problem_number)); }}>
                                                    {submissionResult.problem_number}
                                                </TableCell>
                                                <TableCell style={{ color: getColorByResult('result' in submissionResult ? submissionResult.result : "PD"), width: "20%" }} className="my-table-cell">{'result' in submissionResult ? submissionResult.result : "PD"}</TableCell>
                                                <TableCell style={{ color: isSubmissionResultOwn(submissionResult) ? getColorByScore(submissionResult.general_score) : undefined, width: "20%" }} className="my-table-cell">{isSubmissionResultOwn(submissionResult) ? submissionResult.general_score : "-"}</TableCell>
                                            </TableRow>
                                        )
                                        )
                                    }
                                </TableBody>
                            </Table>
                        </div>
                    </>
            )
        }
    </>;
}

414.55
418.3
