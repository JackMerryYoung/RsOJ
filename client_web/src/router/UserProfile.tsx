import { useEffect, useMemo, useState, lazy, useRef } from "react";

import { useNavigate, useOutletContext, useParams } from "react-router-dom";

import { useTranslation } from "react-i18next";

import { useSelector } from "react-redux";

import { nanoid } from "nanoid";

const PopupDialog = lazy(() => import("./PopupDialog.tsx"));

import * as globals from "../Globals.ts";
import { RootState } from "../store.ts";
import {
    Avatar,
    Button,
    Dropdown,
    Label,
    makeStyles,
    Option,
    Table,
    TableBody,
    TableCell,
    TableHeader,
    TableHeaderCell,
    TableRow,
} from "@fluentui/react-components";
import { ChevronDownRegular, ChevronUpRegular } from "@fluentui/react-icons";

import Highcharts from "highcharts";
import * as HighchartsReactModule from "highcharts-react-official";

const HighchartsReact: any =
    (HighchartsReactModule as any).HighchartsReact ||
    (HighchartsReactModule as any).default ||
    HighchartsReactModule;

const MAX_AVATAR_SIZE_BYTES = 1024 * 1024;

const useStyles = makeStyles({
    page: {
        maxWidth: "70em",
        margin: "0.9em 1em 0 1em",
    },
    profileHeader: {
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        columnGap: "1em",
        rowGap: "0.75em",
        padding: "0 0 0.75em 0",
        borderBottom: "1px solid #e0e0e0",
        flexWrap: "wrap",
    },
    identity: {
        display: "flex",
        alignItems: "center",
        minWidth: 0,
        columnGap: "0.75em",
    },
    identityDetails: {
        display: "flex",
        flexDirection: "column",
        rowGap: "0.2em",
        minWidth: 0,
    },
    username: {
        overflow: "hidden",
        fontSize: "1.15em",
        fontWeight: 600,
        textOverflow: "ellipsis",
        whiteSpace: "nowrap",
    },
    actions: {
        display: "flex",
        alignItems: "center",
        flexWrap: "wrap",
        gap: "0.5em",
        marginLeft: "auto",
    },
    sectionToggle: {
        minWidth: "auto",
    },
    stats: {
        display: "flex",
        alignItems: "stretch",
        flexWrap: "wrap",
        gap: "0.5em",
        marginTop: "0.75em",
    },
    stat: {
        display: "flex",
        alignItems: "baseline",
        columnGap: "0.4em",
        minWidth: "8em",
        padding: "0.45em 0.7em",
        border: "1px solid #e0e0e0",
        borderRadius: "4px",
    },
    statValue: {
        color: "#4183C4",
        fontSize: "1.1em",
        fontWeight: 600,
    },
    statLabel: {
        color: "#666",
        fontSize: "0.9em",
    },
    chart: {
        marginTop: "0.75em",
        padding: "0.75em",
        border: "1px solid #e0e0e0",
        borderRadius: "4px",
        minWidth: 0,
    },
    chartHeader: {
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        columnGap: "0.75em",
        rowGap: "0.5em",
        flexWrap: "wrap",
        paddingBottom: "0.5em",
        borderBottom: "none",
    },
    chartTitle: {
        fontWeight: 600,
    },
    chartBody: {
        minWidth: 0,
        overflow: "hidden",
    },
    chartEmpty: {
        display: "block",
        marginTop: "0.75em",
        color: "#666",
    },
    acceptedProblems: {
        marginTop: "0.75em",
        padding: "0.75em",
        border: "1px solid #e0e0e0",
        borderRadius: "4px",
        minWidth: 0,
        overflowX: "auto",
    },
    acceptedProblemsHeader: {
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        columnGap: "0.75em",
        rowGap: "0.5em",
        flexWrap: "wrap",
        paddingBottom: "0.5em",
        borderBottom: "none",
    },
    acceptedProblemsActions: {
        display: "flex",
        alignItems: "center",
        columnGap: "0.5em",
    },
    acceptedProblemRow: {
        "&:last-child": {
            borderBottomStyle: "none",
        },
    },
    avatarUploader: {
        display: "flex",
        flexDirection: "column",
        alignItems: "flex-start",
        gap: "0.35em",
    },
    error: {
        color: "#DA3737",
        fontSize: "0.9em",
    },
    mobileStack: {
        "@media (max-width: 520px)": {
            width: "100%",
            marginLeft: 0,
        },
    },
});

function AvatarUploader({ username, sessionToken, onUploaded, className }: {
    username: string;
    sessionToken: string;
    onUploaded: () => void;
    className?: string;
}) {
    const { t } = useTranslation("userProfile");
    const styles = useStyles();
    const [error, setError] = useState<string | undefined>(undefined);
    const [uploading, setUploading] = useState(false);
    const fileInputRef = useRef<HTMLInputElement | null>(null);

    const handleFileChange = async (ev: React.ChangeEvent<HTMLInputElement>) => {
        const file = ev.target.files?.[0];
        ev.target.value = "";
        if (!file) return;

        if (file.size > MAX_AVATAR_SIZE_BYTES) {
            setError(t("avatarUploader.fileTooLarge"));
            return;
        }

        setUploading(true);
        setError(undefined);

        const formData = new FormData();
        formData.append("username", username);
        formData.append("session_token", sessionToken);
        formData.append("file", file);

        try {
            const response = await fetch("/avatar/upload", { method: "POST", body: formData });
            if (!response.ok) {
                setError(await response.text());
            } else {
                onUploaded();
            }
        } catch {
            setError(t("avatarUploader.uploadFailed"));
        } finally {
            setUploading(false);
        }
    };

    return <div className={`${styles.avatarUploader} ${className ?? ""}`}>
        <input
            ref={fileInputRef}
            type="file"
            accept="image/png,image/jpeg,image/gif,image/webp"
            style={{ display: "none" }}
            onChange={handleFileChange} />
        <Button
            appearance="secondary"
            disabled={uploading}
            onClick={() => fileInputRef.current?.click()}>
            {uploading ? t("avatarUploader.uploading") : t("avatarUploader.uploadAvatar")}
        </Button>
        {error && <Label className={styles.error}>{error}</Label>}
    </div>;
}

interface UserProfileFromFetcher {
    username: string;
    accepted: number;
    test_accepted: number;
    general: number;
    accepted_submission_timestamps: number[];
    accepted_problems: AcceptedProblem[];
    is_following?: boolean;
    is_followed_by?: boolean;
}

interface AcceptedProblem {
    problem_number: number;
    problem_name: string;
    difficulty: number;
    first_accepted_at: number;
}

type AcceptedProblemSort = "number" | "time" | "difficulty";

type SubmissionChartGranularity = "day" | "month" | "year";

function padDatePart(value: number) {
    return String(value).padStart(2, "0");
}

function getChartBucket(timestamp: number, granularity: SubmissionChartGranularity) {
    const date = new Date(timestamp * 1000);
    const year = date.getFullYear();
    const month = padDatePart(date.getMonth() + 1);
    if (granularity === "year") return String(year);
    if (granularity === "month") return `${year}-${month}`;
    return `${year}-${month}-${padDatePart(date.getDate())}`;
}

function UserSubmissionChart({ timestamps }: { timestamps: number[] }) {
    const { t } = useTranslation("userProfile");
    const styles = useStyles();
    const [granularity, setGranularity] = useState<SubmissionChartGranularity>("day");
    const [isExpanded, setIsExpanded] = useState(true);

    const chartData = useMemo(() => {
        const counts = new Map<string, number>();
        timestamps.forEach((timestamp) => {
            const bucket = getChartBucket(timestamp, granularity);
            counts.set(bucket, (counts.get(bucket) ?? 0) + 1);
        });
        return [...counts.entries()].sort(([left], [right]) => left.localeCompare(right));
    }, [granularity, timestamps]);

    const chartOptions = useMemo<Highcharts.Options>(() => ({
        chart: { type: "line", height: 320, spacing: [12, 8, 8, 8] },
        title: { text: undefined },
        xAxis: {
            categories: chartData.map(([bucket]) => bucket),
            title: { text: t(`chart.granularity.${granularity}`) },
        },
        yAxis: {
            allowDecimals: false,
            min: 0,
            title: { text: t("chart.acceptedCount") },
        },
        tooltip: {
            valueSuffix: ` ${t("chart.submissionsSuffix")}`,
        },
        legend: { enabled: false },
        series: [{
            name: t("chart.acceptedCount"),
            type: "line",
            data: chartData.map(([, count]) => count),
            color: "#0078d4",
            marker: { enabled: true, radius: 3 },
        }],
        credits: { enabled: false },
    }), [chartData, granularity, t]);

    return <section className={styles.chart} aria-label={t("chart.title")}>
        <div className={styles.chartHeader}>
            <Label className={styles.chartTitle}>{t("chart.title")}</Label>
            <div className={styles.acceptedProblemsActions}>
                {isExpanded && <Dropdown
                    value={t(`chart.granularity.${granularity}`)}
                    selectedOptions={[granularity]}
                    onOptionSelect={(_, data) => {
                        if (data.optionValue === "day" || data.optionValue === "month" || data.optionValue === "year") {
                            setGranularity(data.optionValue);
                        }
                    }}
                    aria-label={t("chart.granularityLabel")}
                >
                    <Option value="day">{t("chart.granularity.day")}</Option>
                    <Option value="month">{t("chart.granularity.month")}</Option>
                    <Option value="year">{t("chart.granularity.year")}</Option>
                </Dropdown>}
                <Button
                    appearance="subtle"
                    className={styles.sectionToggle}
                    icon={isExpanded ? <ChevronUpRegular /> : <ChevronDownRegular />}
                    aria-label={t(isExpanded ? "chart.collapse" : "chart.expand")}
                    title={t(isExpanded ? "chart.collapse" : "chart.expand")}
                    aria-expanded={isExpanded}
                    onClick={() => setIsExpanded((expanded) => !expanded)}
                />
            </div>
        </div>
        {isExpanded && (chartData.length > 0
            ? <div className={styles.chartBody}><HighchartsReact highcharts={Highcharts} options={chartOptions} /></div>
            : <Label className={styles.chartEmpty}>{t("chart.noData")}</Label>)}
    </section>;
}

function formatAcceptedAt(timestamp: number) {
    return new Date(timestamp * 1000).toLocaleString();
}

function UserAcceptedProblems({ problems }: { problems: AcceptedProblem[] }) {
    const { t } = useTranslation("userProfile");
    const styles = useStyles();
    const [sortBy, setSortBy] = useState<AcceptedProblemSort>("number");
    const [isExpanded, setIsExpanded] = useState(true);

    const sortedProblems = useMemo(() => [...problems].sort((left, right) => {
        if (sortBy === "time") {
            return left.first_accepted_at - right.first_accepted_at || left.problem_number - right.problem_number;
        }
        if (sortBy === "difficulty") {
            return left.difficulty - right.difficulty || left.problem_number - right.problem_number;
        }
        return left.problem_number - right.problem_number;
    }), [problems, sortBy]);

    return <section className={styles.acceptedProblems} aria-label={t("acceptedProblems.title")}>
        <div className={styles.acceptedProblemsHeader}>
            <Label className={styles.chartTitle}>{t("acceptedProblems.title")}</Label>
            <div className={styles.acceptedProblemsActions}>
                {isExpanded && <Dropdown
                    value={t(`acceptedProblems.sort.${sortBy}`)}
                    selectedOptions={[sortBy]}
                    onOptionSelect={(_, data) => {
                        if (data.optionValue === "number" || data.optionValue === "time" || data.optionValue === "difficulty") {
                            setSortBy(data.optionValue);
                        }
                    }}
                    aria-label={t("acceptedProblems.sortLabel")}
                >
                    <Option value="number">{t("acceptedProblems.sort.number")}</Option>
                    <Option value="time">{t("acceptedProblems.sort.time")}</Option>
                    <Option value="difficulty">{t("acceptedProblems.sort.difficulty")}</Option>
                </Dropdown>}
                <Button
                    appearance="subtle"
                    className={styles.sectionToggle}
                    icon={isExpanded ? <ChevronUpRegular /> : <ChevronDownRegular />}
                    aria-label={t(isExpanded ? "acceptedProblems.collapse" : "acceptedProblems.expand")}
                    title={t(isExpanded ? "acceptedProblems.collapse" : "acceptedProblems.expand")}
                    aria-expanded={isExpanded}
                    onClick={() => setIsExpanded((expanded) => !expanded)}
                />
            </div>
        </div>
        {isExpanded && <>
            {sortedProblems.length === 0
                ? <Label className={styles.chartEmpty}>{t("acceptedProblems.empty")}</Label>
                : <Table size="small">
                    <TableHeader>
                        <TableRow>
                            <TableHeaderCell>{t("acceptedProblems.problemNumber")}</TableHeaderCell>
                            <TableHeaderCell>{t("acceptedProblems.problemName")}</TableHeaderCell>
                            <TableHeaderCell>{t("acceptedProblems.difficulty")}</TableHeaderCell>
                            <TableHeaderCell>{t("acceptedProblems.firstAcceptedAt")}</TableHeaderCell>
                        </TableRow>
                    </TableHeader>
                    <TableBody>
                        {sortedProblems.map((problem) => <TableRow key={problem.problem_number} className={styles.acceptedProblemRow}>
                            <TableCell>{problem.problem_number}</TableCell>
                            <TableCell>{problem.problem_name || "-"}</TableCell>
                            <TableCell>{problem.difficulty}</TableCell>
                            <TableCell>{formatAcceptedAt(problem.first_accepted_at)}</TableCell>
                        </TableRow>)}
                    </TableBody>
                </Table>}
        </>}
    </section>;
}

function useUserProfileLegacy(
    sendJsonMessage: globals.SendJsonMessage,
    lastJsonMessage: unknown,
    targetUsername: string,
    viewerUsername: string | undefined
) {
    const [userProfile, setUserProfile] = useState<UserProfileFromFetcher | undefined>(undefined);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | undefined>(undefined);
    const [websocketMessageHistory, setWebsocketMessageHistory] = useState<unknown[]>([]);
    const [requestKey, setRequestKey] = useState("");
    const requestTimeoutRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

    const fetchUserProfile = () => {
        setLoading(true);
        setError(undefined);
        const nextRequestKey = nanoid();
        sendJsonMessage({
            type: "user_profile",
            content: {
                username: targetUsername,
                viewer_username: viewerUsername,
                request_key: nextRequestKey,
            },
        });
        setRequestKey(nextRequestKey);
        if (requestTimeoutRef.current !== undefined) clearTimeout(requestTimeoutRef.current);
        requestTimeoutRef.current = setTimeout(() => {
            setLoading(false);
            setError("Profile request timed out.");
        }, 8000);
    };

    useEffect(() => {
        if (lastJsonMessage !== null) setWebsocketMessageHistory((history) => history.concat(lastJsonMessage));
    }, [lastJsonMessage]);

    useEffect(() => {
        const remainingMessages = [...websocketMessageHistory];
        let consumedMessage = false;
        remainingMessages.forEach((message, index) => {
            if (typeof message !== "object" || message === null || !("type" in message) || !("content" in message)) return;
            let payload: unknown = message;
            const envelope = message as { type?: unknown; content?: unknown };
            if (envelope.type === "on_send_msg" && typeof envelope.content === "object" && envelope.content !== null) {
                const wrapped = envelope.content as Record<string, unknown>;
                payload = wrapped.msg_to_send;
            }
            if (typeof payload !== "object" || payload === null || !("type" in payload) || !("content" in payload)) return;
            const response = payload as { type?: unknown; content?: unknown };
            if (response.type !== "user_profile" || typeof response.content !== "object" || response.content === null) return;
            const content = response.content as Record<string, unknown>;
            if (content.request_key !== requestKey || typeof content.username !== "string") return;

            setUserProfile({
                username: content.username,
                accepted: typeof content.accepted === "number" ? content.accepted : 0,
                test_accepted: typeof content.test_accepted === "number" ? content.test_accepted : 0,
                general: typeof content.general === "number" ? content.general : 0,
                accepted_submission_timestamps: Array.isArray(content.accepted_submission_timestamps)
                    ? content.accepted_submission_timestamps.filter((timestamp): timestamp is number => typeof timestamp === "number")
                    : [],
                accepted_problems: Array.isArray(content.accepted_problems)
                    ? content.accepted_problems.flatMap((problem): AcceptedProblem[] => {
                        if (typeof problem !== "object" || problem === null) return [];
                        const candidate = problem as Record<string, unknown>;
                        if (typeof candidate.problem_number !== "number" || typeof candidate.difficulty !== "number" || typeof candidate.first_accepted_at !== "number") return [];
                        return [{
                            problem_number: candidate.problem_number,
                            problem_name: typeof candidate.problem_name === "string" ? candidate.problem_name : "",
                            difficulty: candidate.difficulty,
                            first_accepted_at: candidate.first_accepted_at,
                        }];
                    })
                    : [],
                is_following: typeof content.is_following === "boolean" ? content.is_following : undefined,
                is_followed_by: typeof content.is_followed_by === "boolean" ? content.is_followed_by : undefined,
            });
            setLoading(false);
            setError(undefined);
            if (requestTimeoutRef.current !== undefined) clearTimeout(requestTimeoutRef.current);
            remainingMessages[index] = undefined;
            consumedMessage = true;
        });
        if (consumedMessage) setWebsocketMessageHistory(remainingMessages.filter((message) => message !== undefined));
    }, [requestKey, websocketMessageHistory]);

    useEffect(() => () => {
        if (requestTimeoutRef.current !== undefined) clearTimeout(requestTimeoutRef.current);
    }, []);

    return { userProfile, fetchUserProfile, loading, error };
}

function useFollowAction(
    sendJsonMessage: globals.SendJsonMessage,
    lastJsonMessage: unknown,
    onChanged: () => void
) {
    const loginUsername = useSelector((state: RootState) => state.loginUsername);
    const sessionToken = useSelector((state: RootState) => state.sessionToken);
    const [requestKey, setRequestKey] = useState("");
    const [action, setAction] = useState<"follow" | "unfollow" | undefined>(undefined);

    const follow = (targetUsername: string) => {
        const nextRequestKey = nanoid();
        sendJsonMessage({ type: "follow", content: { username: loginUsername.value, session_token: sessionToken.value, target_username: targetUsername, request_key: nextRequestKey } });
        setRequestKey(nextRequestKey);
        setAction("follow");
    };

    const unfollow = (targetUsername: string) => {
        const nextRequestKey = nanoid();
        sendJsonMessage({ type: "unfollow", content: { username: loginUsername.value, session_token: sessionToken.value, target_username: targetUsername, request_key: nextRequestKey } });
        setRequestKey(nextRequestKey);
        setAction("unfollow");
    };

    useEffect(() => {
        if (lastJsonMessage === null) return;
        if (typeof lastJsonMessage !== "object" || lastJsonMessage === null || !("type" in lastJsonMessage) || !("content" in lastJsonMessage)) return;
        const message = lastJsonMessage as { type?: unknown; content?: unknown };
        if ((action !== "follow" && action !== "unfollow") || message.type !== `${action}_result` || typeof message.content !== "object" || message.content === null) return;
        const content = message.content as Record<string, unknown>;
        if (content.request_key !== requestKey) return;
        if (content.success === true) onChanged();
        setAction(undefined);
    }, [action, lastJsonMessage, onChanged, requestKey]);

    return { follow, unfollow };
}

function FollowButton({ userProfile, onClickFollow, onClickUnfollow }: {
    userProfile: UserProfileFromFetcher;
    onClickFollow: () => void;
    onClickUnfollow: () => void;
}) {
    const { t } = useTranslation("userProfile");
    if (userProfile.is_following === undefined) return <></>;

    if (!userProfile.is_following)
        return <Button appearance="primary" onClick={onClickFollow}>{t("follow")}</Button>;

    if (userProfile.is_followed_by)
        return <Button appearance="secondary" onClick={onClickUnfollow}>{t("mutualFollowed")}</Button>;

    return <Button appearance="secondary" onClick={onClickUnfollow}>{t("followed")}</Button>;
}

export default function UserProfile() {
    const styles = useStyles();
    const { username: usernameFromParams } = useParams<{ username?: string }>();
    const { sendJsonMessage, lastJsonMessage } = useOutletContext<globals.WebSocketHook>();
    const loginStatus = useSelector((state: RootState) => state.loginStatus);
    const loginUsername = useSelector((state: RootState) => state.loginUsername);
    const sessionToken = useSelector((state: RootState) => state.sessionToken);
    const targetUsername = usernameFromParams ?? loginUsername.value;
    const [avatarCacheBuster, setAvatarCacheBuster] = useState(0);
    const { userProfile, fetchUserProfile, loading, error } = useUserProfileLegacy(
        sendJsonMessage,
        lastJsonMessage,
        targetUsername,
        loginStatus.value ? loginUsername.value : undefined
    );
    const { follow, unfollow } = useFollowAction(sendJsonMessage, lastJsonMessage, fetchUserProfile);
    const [dialogRequireLoginOpenState, setDialogRequireLoginOpenState] = useState(false);
    const navigate = useNavigate();
    const { t } = useTranslation("userProfile");

    useEffect(() => {
        setDialogRequireLoginOpenState(loginStatus.value === false);

    }, [loginStatus]);

    useEffect(() => {
        if (loginStatus.value === true && targetUsername)
            fetchUserProfile();
    }, [loginStatus, targetUsername]);

    const isOwnProfile = loginStatus.value === true && targetUsername === loginUsername.value;

    return <>
        <>
            {
                userProfile ?
                    <main className={styles.page}>
                        <section className={styles.profileHeader}>
                            <div className={styles.identity}>
                                <Avatar
                                    size={64}
                                    name={userProfile.username}
                                    image={{ src: `/avatar/${userProfile.username}?v=${avatarCacheBuster}` }} />
                                <div className={styles.identityDetails}>
                                    <Label className={styles.username}>{userProfile.username}</Label>
                                </div>
                            </div>
                            <div className={styles.actions}>
                                {!isOwnProfile &&
                                    <FollowButton
                                        userProfile={userProfile}
                                        onClickFollow={() => follow(userProfile.username)}
                                        onClickUnfollow={() => unfollow(userProfile.username)} />}
                                {isOwnProfile &&
                                    <AvatarUploader
                                        username={loginUsername.value}
                                        sessionToken={sessionToken.value}
                                        onUploaded={() => setAvatarCacheBuster((x) => x + 1)}
                                        className={styles.mobileStack} />}
                            </div>
                        </section>
                        <section className={styles.stats} aria-label={t("statsLabel")}>
                            <div className={styles.stat}>
                                <Label className={styles.statValue}>{userProfile.accepted}</Label>
                                <Label className={styles.statLabel}>{t("acceptedStatLabel")}</Label>
                            </div>
                            <div className={styles.stat}>
                                <Label className={styles.statValue}>{userProfile.test_accepted}</Label>
                                <Label className={styles.statLabel}>{t("testAcceptedStatLabel")}</Label>
                            </div>
                            <div className={styles.stat}>
                                <Label className={styles.statValue}>{userProfile.general}</Label>
                                <Label className={styles.statLabel}>{t("generalStatLabel")}</Label>
                            </div>
                        </section>
                        <UserSubmissionChart timestamps={userProfile.accepted_submission_timestamps} />
                        <UserAcceptedProblems problems={userProfile.accepted_problems} />
                    </main>
                    : loginStatus.value === false
                        ? <></>
                        : loading
                        ? <Label className={styles.chartEmpty} role="status">{t("loading")}</Label>
                        : error
                            ? <Label className={styles.error} role="alert">{t("loadFailed")}</Label>
                            : <Label className={styles.chartEmpty} role="status">{t("loading")}</Label>
            }
        </>
        <PopupDialog
            open={dialogRequireLoginOpenState}
            setPopupDialogOpenState={setDialogRequireLoginOpenState}
            text={t("pleaseLoginFirst")}
            onClose={() => navigate("/login")} />
    </>;
}
